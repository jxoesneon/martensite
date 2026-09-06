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
Why are we doing this? What use case does it support? What is the expected outcome? Highlight the specific pain point developers face today when using Martensite. Frame this in terms of the project's Ten Golden Laws if applicable. 

## Guide-level explanation
Explain the proposal as if it was already included in Martensite and you are teaching it to another Rust programmer. That generally means:
- Introducing new named concepts.
- Explaining the feature largely in terms of examples.
- Showing how existing Martensite users would adopt the new feature in their apps.
- If this proposes a new API, provide exact Rust code examples of how a user would consume it. 

## Reference-level explanation
This is the technical specification of the feature. It must be exhaustive and written with extreme precision. 
- State the exact Rust API signatures, type definitions, and trait bounds.
- Specify the exact algorithms, memory layout, and runtime implications.
- Identify all edge cases, error conditions, and platform differences (Windows, macOS, Linux, WebAssembly).
- Enumerate any invariants being added or modified (e.g., changes to the generational arena).
- Provide enough detail that a contributor without prior context could implement the feature identically to your intent. Stubs or `todo!()` placeholders are strictly forbidden.

## Drawbacks
Why should we *not* do this? Provide a brutally honest assessment of the costs. Consider:
- Does it increase compilation time?
- Does it add to resident memory usage (RSS)?
- Does it introduce any allocations in the hot path (breaking Zero-GC)?
- Does it bloat the API surface area?
- Does it make Martensite harder to learn?

## Rationale and alternatives
- Why is this design the best in the space of possible designs?
- What other designs have been considered and what is the rationale for not choosing them? (You must list at least 3 alternatives).
- What is the impact of not doing this?

## Prior art
Discuss prior art, both the good and the bad, in relation to this proposal. You must reference how other native or declarative UI frameworks handle this problem. Specifically, analyze prior art from:
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
- What related issues do you consider out of scope for this RFC that could be addressed in the future independently of the solution that comes out of this RFC?

## Future possibilities
Think about what the natural extension and evolution of your proposal would be and how it would affect the framework as a whole in a major version or two. What does this RFC enable?
