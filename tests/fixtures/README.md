# System parity fixtures

Copied from ducktape core shipping components at commit `c838560bf686aa7890891d9085156889769308fe` (PR #2564), using `crates/modules/system/<id>/component.wasm`. No separate fixture build.

Guest SDK: `b1de6b0bcab9e2a6406ab789388ca262eebf220b`. Native SDK: `22b10d8d10639492b01d34558552b1b53778af0f`. Native host/statesync: core `e6d7b4d26c94ba41cd57d0621745b7761e9bf21d`. The SDK manager explicitly permits this additive first-wave source/guest combination; WIT is unchanged between these SDK revisions.

Refresh from the coordinated frozen core revision for the next guest wave, recording new hashes and source/guest pins.

| File | SHA-256 |
| --- | --- |
| identity.component.wasm | 97f166d8ba684a4be25a9c077d1b74d0dd8232f6e83bb58989fdb33dd07223af |
| dispatch.component.wasm | ee60b47b5a5f2fc8c1adae55a9be9eda516ec8927577f278fd8e43a75ef35b0f |
| attribution.component.wasm | 3027acc033838524b2d06e80fa1fd3d7f5e4d952b8ddcf8cd93ae52e8eece881 |
