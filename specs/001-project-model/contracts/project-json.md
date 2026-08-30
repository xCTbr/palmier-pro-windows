# Contract: `project.json`

**Feature**: 001-project-model

This is the external interface of `palmier-core`: the on-disk format shared with
Palmier Pro on macOS. It is not ours to redesign. The contract is defined by the Swift
decoders audited in [research.md](../research.md); this document states the obligations
that follow.

## Obligations when reading

1. Accept every document Palmier Pro can write, including documents from older
   versions carrying `Transform`'s legacy `x`/`y` keys.
2. Accept the legacy bare-`Timeline` document shape.
3. Apply each field's audited strictness exactly. Where the original tolerates a
   malformed value, tolerate it identically; where it fails, fail.
4. Preserve any key not modeled here, at any depth, so that a newer macOS version's
   data survives a load and save on this side.
5. Reject a document with zero timelines.
6. Fail with the JSON path of the offending value and return no partial project.

## Obligations when writing

1. Emit every key the Swift decoder requires, in the type it expects.
2. Omit absent optional fields rather than writing `null`.
3. Re-emit preserved unknown keys. They land after the keys this project models
   rather than at their original offset, which is sound because key order within a
   JSON object carries no meaning — but it does mean a byte-identical round trip is
   not promised, only a semantically identical one.
4. Never emit `Transform`'s legacy `x`/`y`.
5. Emit frame values as JSON integers and never in exponential notation.

## Compatibility discipline

- The format may only gain optional keys. Renaming, repurposing, or removing a key is
  a breaking change to a format we do not own.
- Upstream continues to evolve. Drift is detected by diffing, not at runtime:

  ```bash
  git fetch upstream
  git diff macos-reference upstream/main -- Sources/PalmierPro/Models/
  ```

- Nothing in this repository can execute the Swift decoder. Every claim about what
  macOS accepts is a code-reading claim, and the round-trip tests prove
  self-consistency, not interoperability. Interoperability is verified by opening a
  written project on a real Mac — a manual step, named here so it is not mistaken for
  something the test suite covers.

## CLI contract: `palmier inspect`

```
palmier inspect <path>
```

`<path>` is a `.palmier` folder or a `project.json`. On success prints, per timeline:
name, fps, resolution, duration in frames and timecode, track count, and per-track
type and clip count. Exit code 0.

On failure prints a diagnostic naming what was wrong — path not found, not a project,
malformed JSON with its location, or a rejected document — and exits non-zero. It
never prints a partial summary for a project that failed to load.
