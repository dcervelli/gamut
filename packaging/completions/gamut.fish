# fish completion for gamut
#
# The option list here is checked against `--help` by a test in src/cli.rs, so
# a flag cannot be added to one without the other.

complete -c gamut -s h -l help        -d 'Show this help'
complete -c gamut -s V -l version     -d 'Show the version'
complete -c gamut      -l output      -d 'Start on an sdr or an hdr surface' -x -a 'sdr hdr'
complete -c gamut      -l transfer    -d 'Override the transfer function' -x -a 'linear srgb pq hlg gamma:'
complete -c gamut      -l primaries   -d 'Override the colour primaries'  -x -a 'bt709 p3 bt2020 adobe'
complete -c gamut      -l no-gain-map -d 'Show the SDR base image of an Ultra HDR JPEG'
complete -c gamut      -l colormap    -d 'False colour on single-channel images' -x -a 'gray viridis magma turbo'
complete -c gamut      -l tone-map    -d 'Start with this tone map'       -x -a 'none reinhard neutral'
complete -c gamut      -l window      -d 'Start with the window set this way' -x -a 'unit minmax pct'
complete -c gamut      -l exposure    -d 'Start at this exposure, in stops' -x
complete -c gamut      -l upscale     -d 'How to resample above 100%'     -x -a 'nearest bicubic'
complete -c gamut      -l histogram   -d 'Start with the histogram showing'
complete -c gamut      -l info        -d 'Start with the file information panel showing'
complete -c gamut      -l no-minimap  -d 'Start with the minimap off'
complete -c gamut      -l timing      -d 'Print decode and startup timings to stdout'
