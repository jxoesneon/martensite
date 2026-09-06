---
rfc: 0000
title: "Feature Name"
status: Draft # Options: Draft, Active, FCP, Accepted, Rejected, Implemented, Stabilized
champion: "Your Name (@github_handle)"
created: YYYY-MM-DD
---

# RFC 0000: Feature Name

## Summary
Provide a brief explanation of the feature. This section must be 5 sentences or fewer. Focus entirely on *what* is changing and the *immediate result* of the change. Do not put extensive justification here.

## Motivation
Why are we doing this? What use case does it support? What is the expected outcome? Highlight the specific requirement or friction developers face today. Frame this in terms of the project's Core Architectural Principles if applicable. 

## Guide-level explanation
Explain the proposal as if it was already included in Martensite and you are teaching it to another Rust programmer. That generally means:
- Introducing new named concepts.
- Explaining the feature largely in terms of examples.
- Showing how existing Martensite users would adopt the new feature in their apps.
- If this proposes a new API, provide exact Rust code examples of how a user would consume it. 

## Reference-level explanation
This is the technical specification of the feature. It should be clear, detailed, and precise:
- State the exact Rust API signatures, type definitions, and trait bounds.
- Specify the algorithms, memory layout, and runtime implications.
- Identify edge cases, error conditions, and platform differences (Windows, macOS, Linux, WebAssembly).
- Enumerate any invariants being added or modified (e.g., changes to the generational arena).
- Provide enough detail that a contributor without prior context could implement the feature cleanly.

## Drawbacks
Why should we *not* do this? Provide an objective assessment of the costs and trade-offs. Consider:
- Does it increase compilation time?
- Does it add to resident memory usage (RSS)?
- Does it introduce any dynamic allocations in the hot path?
- Does it bloat the API surface area?
- Does it add conceptual complexity?

## Rationale and alternatives
- Why is this design preferred among possible designs?
- What other designs have been considered and what is the rationale for not choosing them?
- What is the impact of not doing this?

## Prior art
Discuss prior art in relation to this proposal, exploring both strengths and limitations of existing solutions. Relevant comparisons may include:
- Qt
- Flutter
- React
- egui
- iced
- slint
- Jetpack Compose / SwiftUI

## Unresolved questions
- What parts of the design do you expect to resolve through the RFC process before this gets merged?
- What parts of the design do you expect to resolve through the implementation of this feature before stabilization?
- What related issues do you consider out of scope for this RFC that could be addressed in the future independently?

## Future possibilities
What future opportunities does this design unlock or make possible?
