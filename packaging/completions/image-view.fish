# fish completion for image-view
#
# The option list here is checked against `--help` by a test in src/cli.rs, so
# a flag cannot be added to one without the other.

complete -c image-view -s h -l help        -d 'Show this help'
complete -c image-view -s V -l version     -d 'Show the version'
complete -c image-view      -l hdr         -d 'Use an HDR surface when the display offers one'
complete -c image-view      -l transfer    -d 'Override the transfer function' -x -a 'linear srgb pq hlg gamma:'
complete -c image-view      -l primaries   -d 'Override the colour primaries'  -x -a 'bt709 p3 bt2020 adobe'
complete -c image-view      -l no-gain-map -d 'Show the SDR base image of an Ultra HDR JPEG'
complete -c image-view      -l colormap    -d 'False colour on single-channel images' -x -a 'gray viridis magma turbo'
complete -c image-view      -l tone-map    -d 'Start with this tone map'       -x -a 'clip reinhard neutral'
complete -c image-view      -l window      -d 'Start with the window set this way' -x -a 'unit minmax pct'
complete -c image-view      -l exposure    -d 'Start at this exposure, in stops' -x
complete -c image-view      -l upscale     -d 'How to resample above 100%'     -x -a 'nearest bicubic'
complete -c image-view      -l histogram   -d 'Start with the histogram showing'
complete -c image-view      -l info        -d 'Start with the file information panel showing'
complete -c image-view      -l no-minimap  -d 'Start with the minimap off'
complete -c image-view      -l timing      -d 'Print decode and startup timings to stdout'
