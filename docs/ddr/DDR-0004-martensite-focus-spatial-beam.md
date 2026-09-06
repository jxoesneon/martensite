# Detailed Design Record: DDR-0004
## Title: `martensite-focus` 2D Spatial Beam Engine & Modal Focus Traps

### 1. Architectural Role & Invariants
`martensite-focus` provides 2D spatial keyboard navigation (arrow keys, gamepad D-pad) and accessibility focus management across complex workstation layouts.
* **Invariant 1.1**: 2D directional navigation must use an **Edge-to-Edge Projected-Beam Metric** rather than center-to-center Euclidean distance to prevent erratic jumping across staggered toolbars.
* **Invariant 1.2**: Modal dialogs and popovers must push isolated `FocusScope` stacks that strictly trap navigation within their boundaries. Dismissing the modal restores focus to the invoking trigger widget.

---

### 2. The Edge-to-Edge Projected Beam Distance Formula

Given a source widget bounding box $R_{\text{src}}$ and candidate bounding box $R_{\text{cand}}$ moving in direction $\mathbf{D} \in \{\text{Left}, \text{Right}, \text{Up}, \text{Down}\}$:

1. **Aperture Half-Plane Test**:
   The candidate must reside strictly forward along the direction vector:
   $$d_{\text{major}} = \begin{cases}
   R_{\text{cand}}.\text{min\_x} - R_{\text{src}}.\text{max\_x} & \mathbf{D} = \text{Right} \\
   R_{\text{src}}.\text{min\_x} - R_{\text{cand}}.\text{max\_x} & \mathbf{D} = \text{Left} \\
   R_{\text{cand}}.\text{min\_y} - R_{\text{src}}.\text{max\_y} & \mathbf{D} = \text{Down} \\
   R_{\text{src}}.\text{min\_y} - R_{\text{cand}}.\text{max\_y} & \mathbf{D} = \text{Up}
   \end{cases}$$
   If $d_{\text{major}} \le 0$, the candidate is discarded.

2. **Orthogonal Distance & Projected Overlap**:
   Let $\text{Span}_{\text{src}}$ and $\text{Span}_{\text{cand}}$ be the orthogonal coordinate intervals.
   $$\text{Overlap} = \max\left(0, \min(R_{\text{src}}.\text{max\_ortho}, R_{\text{cand}}.\text{max\_ortho}) - \max(R_{\text{src}}.\text{min\_ortho}, R_{\text{cand}}.\text{min\_ortho})\right)$$

3. **Composite Beam Score (Lowest Score Wins)**:
   $$\text{Score} = (10.0 \cdot d_{\text{major}}) + (35.0 \cdot d_{\text{ortho}}) - 50.0 \cdot \left(\frac{\text{Overlap}}{\min(\text{Span}_{\text{src}}, \text{Span}_{\text{cand}})}\right)$$
