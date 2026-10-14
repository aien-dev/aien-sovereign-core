#define _GNU_SOURCE
#include <dlfcn.h>
#include <fcntl.h>
#include <limits.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <sys/syscall.h>
#include <unistd.h>

static __thread int in_hook = 0;

static void record_path(int dirfd, const char *pathname) {
    if (!pathname || in_hook) return;
    const char *log_path = getenv("MNEME_LOG");
    if (!log_path || !*log_path) return;
    in_hook = 1;

    char resolved[PATH_MAX];
    const char *out = pathname;
    if (pathname[0] != '/' && dirfd == AT_FDCWD) {
        if (getcwd(resolved, sizeof(resolved))) {
            size_t len = strlen(resolved);
            size_t plen = strlen(pathname);
            if (len + 1 + plen < sizeof(resolved)) {
                resolved[len] = '/';
                memcpy(resolved + len + 1, pathname, plen + 1);
                out = resolved;
            }
        }
    }

    const char *root = getenv("MNEME_ROOT");
    if (!root || !*root || strncmp(out, root, strlen(root)) == 0) {
        int fd = (int)syscall(SYS_openat, AT_FDCWD, log_path, O_WRONLY | O_CREAT | O_APPEND | O_CLOEXEC, 0644);
        if (fd >= 0) {
            char buf[PATH_MAX + 2];
            size_t n = strlen(out);
            if (n < PATH_MAX) {
                memcpy(buf, out, n);
                buf[n] = '\n';
                (void)syscall(SYS_write, fd, buf, n + 1);
            }
            (void)syscall(SYS_close, fd);
        }
    }
    in_hook = 0;
}

int open(const char *pathname, int flags, ...) {
    mode_t mode = 0;
    if (flags & (O_CREAT | O_TMPFILE)) {
        va_list ap;
        va_start(ap, flags);
        mode = (mode_t)va_arg(ap, int);
        va_end(ap);
    }
    int r = (int)syscall(SYS_openat, AT_FDCWD, pathname, flags, mode);
    if (r >= 0) record_path(AT_FDCWD, pathname);
    return r;
}

int open64(const char *pathname, int flags, ...) {
    mode_t mode = 0;
    if (flags & (O_CREAT | O_TMPFILE)) {
        va_list ap;
        va_start(ap, flags);
        mode = (mode_t)va_arg(ap, int);
        va_end(ap);
    }
    int r = (int)syscall(SYS_openat, AT_FDCWD, pathname, flags, mode);
    if (r >= 0) record_path(AT_FDCWD, pathname);
    return r;
}

int openat(int dirfd, const char *pathname, int flags, ...) {
    mode_t mode = 0;
    if (flags & (O_CREAT | O_TMPFILE)) {
        va_list ap;
        va_start(ap, flags);
        mode = (mode_t)va_arg(ap, int);
        va_end(ap);
    }
    int r = (int)syscall(SYS_openat, dirfd, pathname, flags, mode);
    if (r >= 0) record_path(dirfd, pathname);
    return r;
}

int openat64(int dirfd, const char *pathname, int flags, ...) {
    mode_t mode = 0;
    if (flags & (O_CREAT | O_TMPFILE)) {
        va_list ap;
        va_start(ap, flags);
        mode = (mode_t)va_arg(ap, int);
        va_end(ap);
    }
    int r = (int)syscall(SYS_openat, dirfd, pathname, flags, mode);
    if (r >= 0) record_path(dirfd, pathname);
    return r;
}

__attribute__((constructor))
static void mneme_init(void) {
    const char *cur = getenv("ASAN_OPTIONS");
    if (!cur || !*cur) {
        setenv("ASAN_OPTIONS", "verify_asan_link_order=0", 1);
    } else if (!strstr(cur, "verify_asan_link_order")) {
        char buf[1024];
        size_t n = strlen(cur);
        if (n + 26 < sizeof(buf)) {
            memcpy(buf, cur, n);
            memcpy(buf + n, ":verify_asan_link_order=0", 26);
            setenv("ASAN_OPTIONS", buf, 1);
        }
    }
}
