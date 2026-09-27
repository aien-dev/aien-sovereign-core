//! Crumb v1 conformance vectors for other implementations (e.g. Omega's reader).

use crate::gen::{self, registry};
use std::path::Path;

/// Write one visible crumb per family (seed 7) plus an expected-values file.
pub fn write_vectors(dir: &Path) -> i32 {
    if std::fs::create_dir_all(dir).is_err() {
        return 1;
    }
    let mut index = String::new();
    for f in &registry().families {
        let g = gen::generate(f, 7);
        let name = format!("v{:03}.crb", f.id);
        if std::fs::write(dir.join(&name), g.visible.to_bytes()).is_err() {
            return 1;
        }
        let mut line = format!(
            "{name} ok enc={} in={}x{} out={}x{} n={}",
            g.visible.encoding as u8,
            g.visible.in_arity,
            g.visible.in_lane_bytes,
            g.visible.out_arity,
            g.visible.out_lane_bytes,
            g.visible.examples.len()
        );
        for (i, o) in g.visible.values() {
            line.push_str(" |");
            for v in i {
                line.push_str(&format!(" {v}"));
            }
            line.push_str(" ->");
            for v in o {
                line.push_str(&format!(" {v}"));
            }
        }
        index.push_str(&line);
        index.push('\n');
    }
    if std::fs::write(dir.join("expected.txt"), index).is_err() {
        return 1;
    }
    0
}
