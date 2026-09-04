# bash completion for gamut
#
# The option list here is checked against `--help` by a test in src/cli.rs, so
# a flag cannot be added to one without the other.

_gamut() {
    local cur prev
    cur=${COMP_WORDS[COMP_CWORD]}
    prev=${COMP_WORDS[COMP_CWORD-1]}

    case $prev in
        --transfer)   COMPREPLY=($(compgen -W 'linear srgb pq hlg gamma:' -- "$cur")); return ;;
        --primaries)  COMPREPLY=($(compgen -W 'bt709 p3 bt2020 adobe' -- "$cur")); return ;;
        --colormap)   COMPREPLY=($(compgen -W 'gray viridis magma turbo' -- "$cur")); return ;;
        --output)     COMPREPLY=($(compgen -W 'sdr hdr' -- "$cur")); return ;;
        --tone-map)   COMPREPLY=($(compgen -W 'none reinhard neutral' -- "$cur")); return ;;
        --window)     COMPREPLY=($(compgen -W 'unit minmax pct' -- "$cur")); return ;;
        --upscale)    COMPREPLY=($(compgen -W 'nearest bicubic' -- "$cur")); return ;;
        --exposure)   return ;;
    esac

    if [[ $cur == -* ]]; then
        COMPREPLY=($(compgen -W '-h --help -V --version --output --transfer --primaries
            --no-gain-map --colormap --tone-map --window --exposure --upscale
            --histogram --info --no-minimap --timing --' -- "$cur"))
        return
    fi

    _filedir '@(jpg|jpeg|jpe|jfif|png|gif|bmp|tif|tiff|webp|avif|heic|heif|hif|ico|hdr|exr|pnm|pbm|pgm|ppm|pam|JPG|JPEG|PNG|GIF|BMP|TIF|TIFF|WEBP|AVIF|HEIC|HEIF|HIF|ICO|HDR|EXR)'
}

complete -F _gamut gamut
