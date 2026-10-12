CacheChecker (Rust edition) v2.0
================================
Single binary (~1.8 MB) to check cache & total files across all disks.
Interactive TUI + CLI + reports (HTML/JSON/MD/CSV) + clean.

Build
-----
  build.bat                    -> target\release\cachechecker.exe
  (uses MSVC from VS 2022 BuildTools; VS 18 on this machine lacks the
   x64 libs, so a plain `cargo build` fails with LNK1104 msvcrt.lib)
  run_cargo.bat test --release -> run the tests

Copy the exe to a folder on PATH so it can be run from anywhere.

TUI (run without arguments)
---------------------------
  cachechecker                 open the dashboard
  cachechecker tui --cache     straight into a quick cache scan
  cachechecker report r.json --tui   open an old report in the TUI

  Home      : disk list + usage gauges, Space = select drive, Enter = full scan,
              C = quick cache scan
  Scanning  : live progress + ETA, file/folder/size/cache counters,
              throughput sparkline, folder being read, Esc = cancel (partial result)
  Results   : 6 tabs
              1 Overview  - stat cards, cache per category, largest caches, gauges
              2 Cache     - full table: Space mark, a mark all safe, c clean,
                            / filter, s sort, f safe only, Enter detail, o open Explorer
              3 Folder    - top folders per root
              4 Extension - extensions by size
              5 Big Files - largest files (o = open folder)
              6 Stats     - file age, size distribution, per drive
              e = export HTML+JSON, r = menu, ? = help

CLI
---
  cachechecker drives
  cachechecker cache [-o r.html] [--min-size 10MB]
  cachechecker scan [-d C D] [-p folder] [-x "*node_modules*"] [-f html json] [--tui] [--cloud]
  cachechecker clean [-c browser dev] [-n "npm*"] [--older-than 7d] [--include-unsafe] [--recycle-bin] [--yes]
  cachechecker clean -r report.json --dry-run
  cachechecker report r.json -f html md csv
  cachechecker compare old.json new.json
  cachechecker signatures

Notes
-----
  - Status "safe" = recreated automatically; "check" = review first (node_modules, Maven, AI models...)
  - Clean removes folder CONTENTS, the parent folder stays. Locked files are skipped.
  - Cloud drives (Google Drive etc.) are not selected by default: slow & may trigger downloads.
  - Run as Administrator to read system folders.
  - Symlinks/junctions are not followed.
  - JSON report format differs from the Python version (not interchangeable).
  - Add new signatures in src/signatures.rs.
