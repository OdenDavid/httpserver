# docs/

Version notes for `rustserver`: one file per version, `V1.md`, `V2.md`, …

Each is written *after* the code works, from the code, and is pinned to a git tag (`v1`,
`v2`, …) so its line numbers stay checkable.

## The shape of every `Vn.md`

1. **One paragraph** — what this version is, and what it deliberately is not.
2. **The diagram** — required, and it comes first, because it is the map the rest of the
   document is the legend for. The same rules every version:
   - from first principles: name the functions and the types, not the concepts
   - every decision branch shown, including the ones that only happen when something goes
     wrong; a failure that is not drawn is a failure nobody will debug
   - every arrow labelled with the value **and its Rust type**
   - every exit labelled with the exact text printed, the stream it goes to, and `exit N`
     where the process ends
   - numbered steps (`①`, `②`, …) so prose can point at a box by number
   - one notation legend, identical in every version, at the top
3. **Where everything is** — item → line number, pinned to the tag.
4. **Decisions and their costs** — what was chosen, what it cost, what would change it.
5. **Real transcripts** — copied from an actual run, never reconstructed.
6. **Experiments** — what to break on purpose before the next version.
7. **What `Vn+1` asks of this code** — the seams the next version will use, and the
   questions it forces.

## How the code is written: two passes

- **First pass** — the version arrives with comments everywhere: every non-obvious line,
  the reasoning, the alternative that was rejected, a long module doc.
- **Second pass** — read it through, then delete most of it. What survives: a one-line `///`
  per item, a short `//!` header, and `//` only where the code cannot say it itself — a
  syscall's behaviour, a kernel rule, a required ordering, a why that would otherwise be
  re-learned the hard way.

`src/main.rs` is the reference for the second pass: the prose lives in `docs/V1.md`, and the
comments that remain are the ones that stop you getting the same surprise twice.

## The rhythm

```sh
cargo test && cargo clippy --all-targets && cargo fmt     # green before anything moves
git add -A && git commit -m "vN: …" && git tag vN && git push --follow-tags
```
