Delegate
========

Inscriptions may nominate a delegate inscription. Requests for the content of
an inscription with a delegate will instead return the content, content type
and content encoding of the delegate. This can be used to cheaply create copies
of an inscription.

### Specification

To create an inscription I with delegate inscription D:

- Create an inscription D.
- Create an inscription I with no content type and empty data. Unlike the ordinals protocol, the doginals protocol interprets the initial OP codes to determine the number of data chunks (subwoofers) an inscription will contain. To ensure interoperability with prior indexer versions, delegates begin as an empty inscription.
- At the end of inscription I, push the delegate specifics. This includes adding the tag 11, represented as OP_PUSH 11, followed by the serialized binary inscription ID of D. The ID should be serialized as a 32-byte TXID.

_NB_ The bytes of a dogecoin transaction ID are reversed in their text
representation, so the serialized transaction ID will be in the opposite order.

### Example

An example of an inscription which delegates to
`3c592c10a5abf33e1897f17013e29c95e4b230e8fb5d070c1c56defe27c27b45i0`:

```
OP_IF
  OP_PUSH "ord"
  OP_PUSH 1  (inscription of 1 chunk for empty data)
  OP_PUSH 0  (no content-type specified)
  OP_PUSH 0  (0 for the last chunk of data)
  OP_PUSH 0  (empty data)
  OP_PUSH 11  (tag 11 for delegates)
  OP_PUSH 0x1457bc227fede561c0c075dfbe830b2e4959ce21370f197183ef3aba5102c593c  (reversed tx id of Inscription D)
OP_ENDIF
```

Note that the value of tag `11` is decimal, not hex.

