//! Damaged documents never crash docc; the damage comes from a small
//! fixed-seed generator.

mod common;

use std::path::{Path, PathBuf};

use common::*;

/// Rng is a xorshift64* generator with a fixed seed.
struct Rng(u64);

impl Rng {
    /// intn is rand.Intn: a number in [0, n).
    fn intn(&mut self, n: usize) -> usize {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33) as usize % n
    }
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        if e.file_type().unwrap().is_dir() {
            walk(&e.path(), out);
        } else {
            out.push(e.path());
        }
    }
}

/// Damaged sources must never crash any command: exit codes stay 0, 1 or 2.
#[test]
fn mutated_sources_do_not_panic() {
    let base = repo().join("testdata/render/basic/docs/src");
    let mut files = Vec::new();
    walk(&base, &mut files);
    files.sort();
    let mut rng = Rng(1);
    // Go: 150 rounds, 20 with -short
    let rounds = 150;
    for i in 0..rounds {
        let work = temp_dir();
        for f in &files {
            let rel = f.strip_prefix(&base).unwrap();
            let mut data = std::fs::read(f).unwrap();
            if rng.intn(3) == 0 {
                // damage about a third of the files
                // Go: for k := 0; k <= rng.Intn(3); k++ draws the bound anew each time
                let mut k = 0;
                while k <= rng.intn(3) {
                    k += 1;
                    match rng.intn(4) {
                        0 => {
                            if !data.is_empty() {
                                let j = rng.intn(data.len());
                                data.remove(j);
                            }
                        }
                        1 => {
                            let j = rng.intn(data.len() + 1);
                            let set = b"{}[]\":,\\\n\x00\xff# `<{";
                            data.insert(j, set[rng.intn(13)]);
                        }
                        2 => data.truncate(rng.intn(data.len() + 1)),
                        _ => {
                            let j = rng.intn(data.len() + 1);
                            data.splice(j..j, b"[[x#y]]{{x#y}}".iter().copied());
                        }
                    }
                }
            }
            let dst = work.join("docs/src").join(rel);
            std::fs::create_dir_all(dst.parent().unwrap()).unwrap();
            std::fs::write(&dst, &data).unwrap();
        }
        for args in [
            &["validate"][..],
            &["validate", "--fix", "--unsafe"],
            &["build"],
            &["build", "--full"],
            &["check", "--format", "json"],
        ] {
            let (code, _, errs) = work.run(args);
            assert!((0..=2).contains(&code), "round {i} {args:?}: exit {code} {errs}");
        }
    }
}
