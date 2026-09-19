# System parity fixtures

Copied from ducktape core shipping components at commit `662fae0c953760c7acd8e0f71234e4178ad33a56` (PR #2657, the refusal-class wave), using `crates/modules/system/<id>/component.wasm`. No separate fixture build.

Guest SDK: `736865710dcfa7c56f9834747287881c1c25d45d`, the same revision this workspace pins. Native host/statesync used by the parity tests: core `d91dbe32c9d6275383f4ae742acf7a3b94beac5b` until the wave merges.

Module WIT SHA-256: `0e0845bedda9e93c4d6ffd5e06a0d0abc9a7468ec2ac4a060cbb38bf4f182280`.

Refresh from canonical committed shipping components whenever the coordinated guest contract changes; record source/artifact revisions and hashes here.

| File | SHA-256 |
| --- | --- |
| identity.component.wasm | 79c1a4874de56168fb33f0590a00f18480a8e798ee9a2fe51d061cbca77f9e84 |
| dispatch.component.wasm | c17e2c4d5d01077720383bb0f83ce5f2468c372eaf51ab5df6d30f051899f6b8 |
| attribution.component.wasm | 67ef9039f0e3c1a8ce427616ccd3aa8bd21139d80458e8184093ad5d8ee79d2d |
