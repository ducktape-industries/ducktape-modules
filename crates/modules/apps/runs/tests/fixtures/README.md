# Runs state v0 fixture

`state_v0` freezes the six-field `__state` encoding at modules dev `5792ddb` (PR #22).
It exists so the `__state` split can prove old state carries over under modules #17.
Regenerate it with `RUNS_WRITE_STATE_V0=1 cargo test -p runs --test state_v0_fixture`.
Normal test runs compare the current encoder with the checked-in bytes and root.
Once the encoding changes, delete the encoder-comparison test.
Keep the install/query test as the carry-over proof for the frozen v0 state.
