# [DDR-0007] Dynamic Theming & Oklab GPU Interpolation Specification

* **Subsystem:** `martensite-theme`
* **Status:** Approved
* **Authors:** Ciel (Specialist Guilds: Graphics, Architecture & UX)
* **Related ADRs:** ADR-0010

## 1. Mathematical Theory & Color Space Topology

Transitions between visual themes (e.g., Light, Dark, High-Contrast Workstation) must interpolate smoothly without perceptual brightness dips or chromatic aberrations.
Standard sRGB and linear RGB color blending suffer from non-linear perceptual luminance shifts (e.g., blue-yellow interpolation passing through muddy grey or sickly purple).

Martensite mandates that all theme color blending operates in the **Oklab color space**, a perceptually uniform color space designed by Björn Ottosson (2020).

### 1.1 Transformation Pipeline
Given an sRGB triplet $(R, G, B) \in [0, 1]^3$:
1. Convert non-linear sRGB to linear sRGB:
   $$C_{\text{linear}} = \begin{cases} \frac{C_{\text{srgb}}}{12.92} & C_{\text{srgb}} \le 0.04045 \\ \left(\frac{C_{\text{srgb}} + 0.055}{1.055}\right)^{2.4} & C_{\text{srgb}} > 0.04045 \end{cases}$$
2. Transform linear RGB to LMS cone responses via matrix $M_1$:
   $$\begin{bmatrix} L \\ M \\ S \end{bmatrix} = \begin{bmatrix} 0.4122214708 & 0.5363325363 & 0.0514459929 \\ 0.2119034982 & 0.6806995451 & 0.1073969566 \\ 0.0883024619 & 0.2817188376 & 0.6299787005 \end{bmatrix} \begin{bmatrix} R_{\text{linear}} \\ G_{\text{linear}} \\ B_{\text{linear}} \end{bmatrix}$$
3. Apply non-linear cube root compression:
   $$l = L^{1/3}, \quad m = M^{1/3}, \quad s = S^{1/3}$$
4. Map compressed LMS to Oklab coordinates $(L, a, b)$ via matrix $M_2$:
   $$\begin{bmatrix} L \\ a \\ b \end{bmatrix} = \begin{bmatrix} 0.2104542553 & 0.7936177850 & -0.0040720468 \\ 1.9779984951 & -2.4285922050 & 0.4505937099 \\ 0.0259040371 & 0.7827717662 & -0.8086757660 \end{bmatrix} \begin{bmatrix} l \\ m \\ s \end{bmatrix}$$

### 1.2 Invariant Guarantees
* **Invariant 1.1 (Perceptual Uniformity)**: Euclidean distance in Oklab approximates perceptual color difference $\Delta E_{\text{ok}} = \sqrt{\Delta L^2 + \Delta a^2 + \Delta b^2}$.
* **Invariant 1.2 (Zero CPU Reflow on Theme Switch)**: Changing themes alters GPU uniform buffers only. No CPU layout re-measurement or node tree traversal occurs.
