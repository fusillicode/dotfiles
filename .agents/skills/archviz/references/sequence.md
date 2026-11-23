# UML sequence diagrams

- Box participants above dashed lifelines. Order messages top to bottom, from sender to receiver.
- Use `.call` for solid synchronous calls with filled heads, the default for solid asynchronous messages with open heads, and `.reply` for dashed replies. Add `.reverse` for right-to-left messages.
- Set `--columns` and `--span` to cover both participants. For payloads, set `--start` to the leftmost lane. Keep lane widths and arrow endpoints fixed as disclosures or options change.
- For self-messages use `.self`, the sender's lane for `--columns` and `--start`, and `--span: 1`. Reuse the usual label and payload markup. The loop returns to the sender's lifeline.
- Show execution bars only for known intervals. Use guarded `alt`, `opt`, or `loop` frames where helpful. Give nested frames separate outlines containing their guards. Keep borders independent of lane and payload widths. Label uncertain message semantics.

[OMG UML 2.5.1, chapter 17](https://www.omg.org/spec/UML/2.5.1/PDF)
