# Chat frozen-wire fixtures

These fixtures were captured from the pre-move `chat-wire` and `chat-message`
packages at SDK source `b66f47f1f4b0c869786ce195e382f2e83fd15277`.

`request-*`, `query-*`, `reply-*`, `event-*`, and `assigned-*` use the SDK
`sdk::wire` serde-JSON codec. `store-channel-json.hex` is the module's
serde-JSON stored record. `party-account-borsh.hex` is the Borsh party bytes
used in composite storage keys. `index-message-query-json.hex` is the index
view's serde-JSON request. The golden test covers one representative value for
each supported direction, plus a trailing-byte malformed request.
