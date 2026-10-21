ProRAW histograms build on aspects of both the [HDR](#hdr-histogram) and [RAW histograms](#raw-histogram) with the addition of a *Profile* section, which describes how a *Developed* pixel is adjusted before *Display* modifications. These are the steps for a given pixel (shown when hovering over the image):

| Step | Description | 
|--|--|
| Input | The value of the pixel after the *Develop* section is the starting point |
| ProPhoto conversion | Convert developed values to linear ProPhoto |
| Apply exposure | Adds the profile's baseline exposure value |
| Apply gain table | Applies the relevant gain curve (separate curves distributed on a grid) to the pixel |
| Apply tone curve | Applies the profile's tone curve |

The output of all the steps is summarized in the *Total lift* line and then a histogram is shown of the gain across the whole image. The application of the profile can be toggled with the icon on the right of the section. When the profile is showing (the default), the RAW should look very similar to the embedded JPEG (view with the button at the bottom or `v`).
