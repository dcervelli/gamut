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
        --primaries)  COMPREPLY=($(compgen -W 'bt709 p3 bt2020 adobe prophoto' -- "$cur")); return ;;
        --colormap)   COMPREPLY=($(compgen -W 'gray viridis magma turbo' -- "$cur")); return ;;
        --output)     COMPREPLY=($(compgen -W 'sdr hdr' -- "$cur")); return ;;
        --tone-map)   COMPREPLY=($(compgen -W 'none neutral' -- "$cur")); return ;;
        --window)     COMPREPLY=($(compgen -W 'stored full trimmed' -- "$cur")); return ;;
        --upscale)    COMPREPLY=($(compgen -W 'nearest bicubic' -- "$cur")); return ;;
        --exposure)   return ;;
        --size)       return ;;
    esac

    if [[ $cur == -* ]]; then
        COMPREPLY=($(compgen -W '-h --help -V --version --print-config --output --transfer --primaries
            --no-gain-map --colormap --tone-map --window --exposure --upscale
            --size --histogram --info --no-minimap --paused --paste --timing --' -- "$cur"))
        return
    fi

    _filedir '@(jpg|jpeg|jpe|jfif|png|gif|bmp|tif|tiff|webp|jxl|avif|heic|heif|hif|ico|hdr|exr|pnm|pbm|pgm|ppm|pam|dng|nef|nrw|cr2|cr3|crw|arw|srf|sr2|raf|orf|rw2|rwl|pef|srw|3fr|fff|iiq|mef|mos|erf|dcr|kdc|mrw|JPG|JPEG|PNG|GIF|BMP|TIF|TIFF|WEBP|JXL|AVIF|HEIC|HEIF|HIF|ICO|HDR|EXR|DNG|NEF|NRW|CR2|CR3|CRW|ARW|SRF|SR2|RAF|ORF|RW2|RWL|PEF|SRW|3FR|FFF|IIQ|MEF|MOS|ERF|DCR|KDC|MRW)'
}

complete -F _gamut gamut
