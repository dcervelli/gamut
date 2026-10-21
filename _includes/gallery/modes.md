Whenever an image file is opened, gamut chooses whether it should be in *Photo* or *Data* mode. The table below summarizes the differences between the modes. In the rare event that gamut chooses poorly or you want to experiment with the other mode, you can change it in the histogram, info panel or with the `|` key.

| Photo | Data |
| --- | --- |
| Values are light, with 1.0 as white (graded) or no stated white (scene) | Values are raw numbers (likely not light at all) |
| Samples use the file's curve (sRGB if none) | Read as linear |
| Scene-referred files (EXR, Radiance) open metered, with key at middle gray | No metering |
| Opens with dark→light window of 0..1 ("As stored") | Opens with dark→light window trimmed to the central 99.8% |
| Histogram's Display section has *Exposure* and *Curve* rows | Histogram's Display section has only the *Window* row |
| No window rules | Uses *Window* rules: (*As stored* / *Full range* / *Trimmed*) |
| Uses exposure adjustment | No exposure adjustment; use histogram white handle instead |
| Uses the tone curve: *Clip* / *Roll off* | No curve |
| Values above white go to the curve and HDR headroom | Values clipped at the window's top, even on HDR |
| Single-channel gray image is shown as light (unless false color is chosen) | Single-channel gray image spread linearly over ramp |
| Linear light plotted in stops counting from white | Plotted on its curve or codes (no stops) |
| Graded file's histogram axis always reaches at least 1.0 | Axis ends at the data's maximum |
