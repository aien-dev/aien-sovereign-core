/*
 * PEARL Cortex path context, Milestone 1: synthetic snapshot + fixture generator.
 *
 * Writes two files:
 *   argv[1]  SQL data (spaces, entities, entities_fts, claims, claim_evidence,
 *            security_state) for a Cortex schema at user_version 5
 *   argv[2]  eval fixture JSON in the camelCase format of cortex-path-m1
 *
 * Everything is derived from one fixed seed. No real memory, no sealed material.
 * The security_state HMAC key is a fixed, public, all-zero test value.
 *
 * World model (frozen before any M1 run; see ../../PEARL_CORTEX_PATH_CONTEXT_M1.md):
 *   - 60 hubs ("Project <Word>"); the query for a hub is its lowercase word.
 *   - each hub has one "<Word> roadmap" entity (name contains the word),
 *     2..5 associates (people and notes; name and content never contain the word),
 *     and in 1 of 3 hubs a "meeting notes" entity that mentions the word in its
 *     content but is not relevant (a lexical hard negative).
 *   - 120 filler entities with no hub relation, 30 entities in a second space.
 *   - associates are only weakly similar to their hub in embedding space.
 *   - true hub/associate relations become claims with probability 0.75; a
 *     further 0.15 are reachable only through another associate (two hops);
 *     the remaining 0.10 have no claim path at all (incomplete memory).
 *   - noise: 180 random active claims between unrelated entities, plus 70
 *     inadmissible claims (retracted, superseded, disputed, invalidated,
 *     literal-only, self-loop, cross-space), some of which touch hubs.
 *   - claim confidence, tier, evidence count and age are drawn at random.
 *   - cases 0..29 ask for everything related to the hub (hub, roadmap,
 *     associates): relational. cases 30..59 ask for the named things only
 *     (hub, roadmap): lexical.
 */
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define DIM 24
#define TOPICS 12
#define HUBS 60
#define RELATIONAL_CASES 30
#define FILLERS 120
#define OTHER_SPACE 30
#define NOISE_EDGES 180
#define BAD_EDGES 70
#define MAX_ENT 1000
#define MAX_ASSOC 5
#define SEED 0x504541524C4D31ULL /* "PEARLM1" */
#define SNAPSHOT_EPOCH 1790726400LL /* 2026-09-30T00:00:00Z */

static uint64_t rng_state = SEED;

static uint64_t next_u64(void) {
    uint64_t z = (rng_state += 0x9E3779B97F4A7C15ULL);
    z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9ULL;
    z = (z ^ (z >> 27)) * 0x94D049BB133111EBULL;
    return z ^ (z >> 31);
}

static double uniform(void) { return (double)(next_u64() >> 11) / 9007199254740992.0; }

static int below(int n) { return (int)(uniform() * n); }

static double gaussian(void) {
    double u1 = uniform(), u2 = uniform();
    if (u1 < 1e-300) u1 = 1e-300;
    return sqrt(-2.0 * log(u1)) * cos(6.283185307179586 * u2);
}

static void random_unit(double *v) {
    double n = 0;
    for (int i = 0; i < DIM; i++) { v[i] = gaussian(); n += v[i] * v[i]; }
    n = sqrt(n);
    for (int i = 0; i < DIM; i++) v[i] /= n;
}

static void normalize(double *v) {
    double n = 0;
    for (int i = 0; i < DIM; i++) n += v[i] * v[i];
    n = sqrt(n);
    for (int i = 0; i < DIM; i++) v[i] /= n;
}

/* out = normalize(a*x + b*y + c*fresh random unit) */
static void mix(double *out, double a, const double *x, double b, const double *y, double c) {
    double u[DIM];
    random_unit(u);
    for (int i = 0; i < DIM; i++)
        out[i] = a * (x ? x[i] : 0) + b * (y ? y[i] : 0) + c * u[i];
    normalize(out);
}

typedef struct {
    char id[16];
    char space[24];
    char type[16];
    char name[96];
    char content[256];
    double emb[DIM];
} Entity;

static Entity ents[MAX_ENT];
static int n_ents = 0;

static Entity *add_entity(const char *space, const char *type) {
    if (n_ents >= MAX_ENT) { fprintf(stderr, "too many entities\n"); exit(1); }
    Entity *e = &ents[n_ents];
    snprintf(e->id, sizeof e->id, "ent-%04d", n_ents);
    snprintf(e->space, sizeof e->space, "%s", space);
    snprintf(e->type, sizeof e->type, "%s", type);
    n_ents++;
    return e;
}

static const char *SYL[] = {"ka", "lo", "mi", "ve", "tor", "na", "shi", "ru", "pel", "dax",
                            "qui", "bo", "zen", "fa", "gri", "hu", "jo", "wex", "yam", "cel"};
#define NSYL (int)(sizeof SYL / sizeof SYL[0])

static char words[HUBS + 400][24];
static int n_words = 0;

/* Pseudo-word of 3 syllables that is not a substring of, and does not contain,
 * any word issued so far. Hub words are issued first, then person-name words. */
static const char *new_word(void) {
    for (;;) {
        char w[24];
        snprintf(w, sizeof w, "%s%s%s", SYL[below(NSYL)], SYL[below(NSYL)], SYL[below(NSYL)]);
        int clash = 0;
        for (int i = 0; i < n_words && !clash; i++)
            if (strstr(words[i], w) || strstr(w, words[i])) clash = 1;
        if (clash) continue;
        snprintf(words[n_words], sizeof words[0], "%s", w);
        return words[n_words++];
    }
}

static void capitalize(char *dst, size_t n, const char *w) {
    snprintf(dst, n, "%s", w);
    if (dst[0] >= 'a' && dst[0] <= 'z') dst[0] -= 32;
}

typedef struct {
    int hub, roadmap, mention;
    int assoc[MAX_ASSOC];
    int n_assoc;
    const char *word;
} Hub;

static Hub hubs[HUBS];

typedef struct {
    int subj, obj; /* obj < 0 means literal-only claim */
    const char *space;
    const char *status;
    int retracted;
} Claim;

static FILE *sql;
static int n_claims = 0;
static int n_admitted_planned = 0;

static const char *TIERS[] = {"T0Direct", "T1Corroborated", "T2Verified", "T3Controlled"};

static void emit_claim(Claim c) {
    char id[16];
    snprintf(id, sizeof id, "clm-%05d", n_claims++);
    double conf = 0.5 + 0.5 * uniform();
    double t = uniform();
    const char *tier = t < 0.45 ? TIERS[0] : t < 0.75 ? TIERS[1] : t < 0.93 ? TIERS[2] : TIERS[3];
    int evidence = below(4);
    long long age_days = below(721);
    time_t created = (time_t)(SNAPSHOT_EPOCH - age_days * 86400LL - below(86400));
    struct tm tmv;
    gmtime_r(&created, &tmv);
    char ts[32];
    strftime(ts, sizeof ts, "%Y-%m-%dT%H:%M:%SZ", &tmv);
    const char *space_id = strcmp(c.space, "atlas-memory") == 0 ? "space-atlas-memory" : "space-other";
    const char *preds[] = {"works_on", "documents", "depends_on", "mentions", "reviewed", "owns"};
    const char *pred = preds[below(6)];
    if (c.obj >= 0) {
        fprintf(sql,
                "INSERT INTO claims (id, space_id, space_slug, subject_entity_id, predicate, "
                "object_entity_id, literal_value_json, confidence, metadata_json, retracted, "
                "created_at, verification_tier, status) VALUES ('%s','%s','%s','%s','%s','%s',NULL,"
                "%.6f,'{}',%d,'%s','%s','%s');\n",
                id, space_id, c.space, ents[c.subj].id, pred, ents[c.obj].id, conf, c.retracted, ts,
                tier, c.status);
    } else {
        fprintf(sql,
                "INSERT INTO claims (id, space_id, space_slug, subject_entity_id, predicate, "
                "object_entity_id, literal_value_json, confidence, metadata_json, retracted, "
                "created_at, verification_tier, status) VALUES ('%s','%s','%s','%s','%s',NULL,"
                "'\"literal\"',%.6f,'{}',%d,'%s','%s','%s');\n",
                id, space_id, c.space, ents[c.subj].id, pred, conf, c.retracted, ts, tier,
                c.status);
    }
    for (int k = 0; k < evidence; k++)
        fprintf(sql,
                "INSERT INTO claim_evidence (claim_id, evidence_id, source_kind, evidence_hash) "
                "VALUES ('%s','ev-%s-%d','synthetic','sha256:%016llx');\n",
                id, id, k, (unsigned long long)next_u64());
}

static void active_edge(int a, int b) {
    Claim c = {a, b, "atlas-memory", "active", 0};
    if (uniform() < 0.5) { c.subj = b; c.obj = a; }
    emit_claim(c);
    n_admitted_planned++;
}

static int random_atlas_entity(void) {
    for (;;) {
        int i = below(n_ents);
        if (strcmp(ents[i].space, "atlas-memory") == 0) return i;
    }
}

static int random_other_entity(void) {
    for (;;) {
        int i = below(n_ents);
        if (strcmp(ents[i].space, "atlas-memory") != 0) return i;
    }
}

static void emit_blob(const double *v) {
    fputs("X'", sql);
    for (int i = 0; i < DIM; i++) {
        float f = (float)v[i];
        uint32_t bits;
        memcpy(&bits, &f, 4);
        fprintf(sql, "%02x%02x%02x%02x", bits & 0xff, (bits >> 8) & 0xff, (bits >> 16) & 0xff,
                (bits >> 24) & 0xff);
    }
    fputs("'", sql);
}

static int contains_ci(const char *hay, const char *needle) {
    size_t n = strlen(needle);
    for (const char *p = hay; *p; p++) {
        size_t i = 0;
        while (i < n && p[i]) {
            char a = p[i], b = needle[i];
            if (a >= 'A' && a <= 'Z') a += 32;
            if (b >= 'A' && b <= 'Z') b += 32;
            if (a != b) break;
            i++;
        }
        if (i == n) return 1;
    }
    return 0;
}

int main(int argc, char **argv) {
    if (argc != 3) { fprintf(stderr, "usage: %s DATA.sql FIXTURE.json\n", argv[0]); return 1; }
    sql = fopen(argv[1], "w");
    FILE *fx = fopen(argv[2], "w");
    if (!sql || !fx) { perror("open"); return 1; }

    double topic[TOPICS][DIM];
    for (int t = 0; t < TOPICS; t++) random_unit(topic[t]);

    for (int h = 0; h < HUBS; h++) hubs[h].word = new_word();

    /* hubs, roadmaps, associates, mentions */
    int person_no = 0, note_no = 0, meeting_no = 0;
    for (int h = 0; h < HUBS; h++) {
        Hub *H = &hubs[h];
        char Word[24];
        capitalize(Word, sizeof Word, H->word);
        int t = below(TOPICS);

        Entity *e = add_entity("atlas-memory", "project");
        H->hub = n_ents - 1;
        snprintf(e->name, sizeof e->name, "Project %s", Word);
        snprintf(e->content, sizeof e->content, "Project %s is an internal engineering effort.", Word);
        mix(e->emb, 0.6, topic[t], 0, NULL, 0.8);

        e = add_entity("atlas-memory", "document");
        H->roadmap = n_ents - 1;
        snprintf(e->name, sizeof e->name, "%s roadmap", Word);
        snprintf(e->content, sizeof e->content, "Milestones and dates for the %s work.", Word);
        mix(e->emb, 0.7, ents[H->hub].emb, 0.3, topic[t], 0.6);

        H->n_assoc = 2 + below(MAX_ASSOC - 1);
        for (int a = 0; a < H->n_assoc; a++) {
            int own_topic = uniform() < 0.5 ? t : below(TOPICS);
            if (uniform() < 0.5) {
                e = add_entity("atlas-memory", "person");
                char f[24], l[24];
                capitalize(f, sizeof f, new_word());
                capitalize(l, sizeof l, new_word());
                snprintf(e->name, sizeof e->name, "%s %s", f, l);
                snprintf(e->content, sizeof e->content, "Engineer, profile %d.", ++person_no);
            } else {
                e = add_entity("atlas-memory", "note");
                snprintf(e->name, sizeof e->name, "Design note %d", ++note_no);
                snprintf(e->content, sizeof e->content,
                         "Notes on an interface decision, record %d.", note_no);
            }
            H->assoc[a] = n_ents - 1;
            mix(e->emb, 0.35, ents[H->hub].emb, 0.5, topic[own_topic], 0.75);
        }

        H->mention = -1;
        if (h % 3 == 0) {
            e = add_entity("atlas-memory", "note");
            H->mention = n_ents - 1;
            snprintf(e->name, sizeof e->name, "Meeting notes %d", ++meeting_no);
            snprintf(e->content, sizeof e->content,
                     "Weekly sync; %s came up briefly among other topics.", Word);
            mix(e->emb, 0.6, topic[below(TOPICS)], 0, NULL, 0.8);
        }
    }

    for (int i = 0; i < FILLERS; i++) {
        Entity *e = add_entity("atlas-memory", i % 2 ? "note" : "task");
        snprintf(e->name, sizeof e->name, "%s %d", i % 2 ? "Reference note" : "Task", i + 1);
        snprintf(e->content, sizeof e->content, "Unrelated background record number %d.", i + 1);
        mix(e->emb, 0.6, topic[below(TOPICS)], 0, NULL, 0.8);
    }
    for (int i = 0; i < OTHER_SPACE; i++) {
        Entity *e = add_entity("other-space", "note");
        snprintf(e->name, sizeof e->name, "Other space record %d", i + 1);
        snprintf(e->content, sizeof e->content, "Belongs to a different memory space.");
        mix(e->emb, 0.6, topic[below(TOPICS)], 0, NULL, 0.8);
    }

    /* self-check: a hub word appears only in its hub, roadmap and mention entity */
    for (int h = 0; h < HUBS; h++) {
        for (int i = 0; i < n_ents; i++) {
            int hit = contains_ci(ents[i].name, hubs[h].word) || contains_ci(ents[i].content, hubs[h].word);
            int allowed = i == hubs[h].hub || i == hubs[h].roadmap || i == hubs[h].mention;
            if (hit != allowed) {
                fprintf(stderr, "word %s placement error at %s\n", hubs[h].word, ents[i].id);
                return 1;
            }
            if (strcmp(ents[i].name, ents[i].id) == 0) return 1;
        }
    }

    fputs("BEGIN;\n", sql);
    fputs("INSERT INTO spaces (id, slug, created_at) VALUES "
          "('space-atlas-memory','atlas-memory','2026-01-01T00:00:00Z'),"
          "('space-other','other-space','2026-01-01T00:00:00Z');\n", sql);
    fputs("UPDATE security_state SET hmac_key = zeroblob(32) WHERE id = 1;\n", sql);
    for (int i = 0; i < n_ents; i++) {
        Entity *e = &ents[i];
        const char *space_id = strcmp(e->space, "atlas-memory") == 0 ? "space-atlas-memory" : "space-other";
        fprintf(sql,
                "INSERT INTO entities (id, space_id, space_slug, entity_type, canonical_name, content, "
                "created_at, embedding) VALUES ('%s','%s','%s','%s','%s','%s','2026-01-01T00:00:00Z',",
                e->id, space_id, e->space, e->type, e->name, e->content);
        emit_blob(e->emb);
        fputs(");\n", sql);
        fprintf(sql,
                "INSERT INTO entities_fts (id, canonical_name, content, space_slug) VALUES "
                "('%s','%s','%s','%s');\n",
                e->id, e->name, e->content, e->space);
    }

    /* true relations */
    for (int h = 0; h < HUBS; h++) {
        Hub *H = &hubs[h];
        active_edge(H->hub, H->roadmap);
        for (int a = 0; a < H->n_assoc; a++) {
            double r = uniform();
            if (r < 0.75) {
                active_edge(H->hub, H->assoc[a]);
            } else if (r < 0.90 && a > 0) {
                active_edge(H->assoc[below(a)], H->assoc[a]);
            } /* else: no path, the memory is incomplete */
        }
    }
    /* active noise between unrelated entities */
    for (int i = 0; i < NOISE_EDGES; i++) {
        int a = random_atlas_entity(), b = random_atlas_entity();
        if (a == b) { i--; continue; }
        active_edge(a, b);
    }
    /* inadmissible claims; half of them touch a hub */
    for (int i = 0; i < BAD_EDGES; i++) {
        int a = (i % 2 == 0) ? hubs[below(HUBS)].hub : random_atlas_entity();
        int b = random_atlas_entity();
        if (a == b) b = (b + 1) % n_ents;
        Claim c = {a, b, "atlas-memory", "active", 0};
        switch (i % 7) {
        case 0: c.retracted = 1; c.status = "superseded"; break;
        case 1: c.status = "superseded"; break;
        case 2: c.status = "disputed"; break;
        case 3: c.status = "invalidated"; break;
        case 4: c.obj = -1; break;
        case 5: c.obj = a; break;
        case 6: c.obj = random_other_entity(); break;
        }
        emit_claim(c);
    }
    /* claims wholly inside the other space */
    for (int i = 0; i < 20; i++) {
        int a = random_other_entity(), b = random_other_entity();
        if (a == b) { i--; continue; }
        Claim c = {a, b, "other-space", "active", 0};
        emit_claim(c);
    }
    fputs("COMMIT;\n", sql);

    /* fixture */
    fputs("{\n  \"kind\": \"synthetic-independent-v1\",\n  \"cases\": [\n", fx);
    for (int h = 0; h < HUBS; h++) {
        Hub *H = &hubs[h];
        double q[DIM];
        mix(q, 0.8, ents[H->hub].emb, 0, NULL, 0.6);
        int relational = h < RELATIONAL_CASES;
        fprintf(fx, "    {\n      \"id\": \"m1-%s-%02d\",\n", relational ? "rel" : "lex", h);
        fprintf(fx, "      \"spaceSlug\": \"atlas-memory\",\n      \"query\": \"%s\",\n", H->word);
        fprintf(fx, "      \"relevantEntityIds\": [\"%s\", \"%s\"", ents[H->hub].id, ents[H->roadmap].id);
        if (relational)
            for (int a = 0; a < H->n_assoc; a++) fprintf(fx, ", \"%s\"", ents[H->assoc[a]].id);
        fputs("],\n      \"queryEmbedding\": [", fx);
        for (int i = 0; i < DIM; i++) fprintf(fx, "%s%.7f", i ? ", " : "", (double)(float)q[i]);
        fprintf(fx, "]\n    }%s\n", h + 1 < HUBS ? "," : "");
    }
    fputs("  ]\n}\n", fx);

    fclose(sql);
    fclose(fx);
    fprintf(stderr, "entities=%d claims=%d admitted_planned=%d cases=%d relational=%d\n", n_ents,
            n_claims, n_admitted_planned, HUBS, RELATIONAL_CASES);
    return 0;
}
