# UML sequence diagrams

- Order `.participant` elements by lane and `.exchange` elements by time. Set `--actors` to the participant count.
- Use `.message` for asynchronous messages, `.call` for synchronous calls, and `.reply` for replies. Add `.reverse` for right-to-left messages.
- Set `--columns` and `--span` to cover both participants. Set `--start` to the leftmost payload lane. For self-messages, add `.self` to the exchange and use the sender's lane with `--span: 1`.
- Show execution bars only for known intervals. Give each `alt`, `opt`, or `loop` frame its own guard. Keep frame wrappers separate from the lane grid. Label uncertain message semantics.

[OMG UML 2.5.1, chapter 17](https://www.omg.org/spec/UML/2.5.1/PDF)
