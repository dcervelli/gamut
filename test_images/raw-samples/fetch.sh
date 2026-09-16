#!/bin/bash
# Fetches one real camera file of each raw format into this directory, for
# `decode::raw`'s sample test:
#
#   ./fetch.sh
#   cargo test --release raw::tests::samples -- --ignored --nocapture
#
# The files are not in the tree. A camera's file is tens of megabytes and
# would be in every clone for ever; and what the test checks is not their
# pixels but that each container is recognized as a raw, probes to the size
# it develops to, develops, hands back its preview the right way up, and
# gives up its metadata — which any file of the format shows. Everything
# below is from raw.pixls.us, where photographers have put one file of nearly
# every camera made under CC0, and each is the smallest file of its camera's
# directory there at the time it was chosen. Everything in this directory but
# this script is ignored by git.
#
# What the tree does carry is `dng-cfa.dng`, the one raw anything but a
# camera can write: that is the fixture, and it runs with every `cargo test`.
set -euo pipefail
cd "$(dirname "$0")"

base="https://raw.pixls.us/data"
fetch() {  # <directory on the server> <file there> <name here>
  [ -e "$3" ] && return
  echo "$3"
  curl -sSL -f -o "$3" "$base/$(python3 -c 'import urllib.parse,sys; print(urllib.parse.quote(sys.argv[1]))' "$1/$2")"
}

fetch "Canon/EOS 300D"            "CRW_8395.CRW"                              "Canon_EOS_300D.CRW"
fetch "Canon/EOS 5D Mark IV"      "B13A0732.CR2"                              "Canon_EOS_5D_Mark_IV.CR2"
fetch "Canon/EOS R"               "Canon_EOS_R_CRAW_ISO_100_crop_nodual.CR3"  "Canon_EOS_R.CR3"
fetch "Fujifilm/X-T2"             "20170114_174341_TFW04727.RAF"              "Fujifilm_X-T2.RAF"
fetch "Hasselblad/X1D"            "B9999910.3FR"                              "Hasselblad_X1D.3FR"
fetch "Leica/M10"                 "f5381888.dng"                              "Leica_M10.dng"
fetch "Minolta/ALPHA-7 DIGITAL"   "PICT6107.MRW"                              "Minolta_Alpha-7_Digital.MRW"
fetch "Nikon/D750"                "compressed_12_bit.NEF"                     "Nikon_D750.NEF"
fetch "Olympus/E-5"               "_7061961_copy.ORF"                         "Olympus_E-5.ORF"
fetch "Panasonic/DC-GH5"          "_T012010.RW2"                              "Panasonic_DC-GH5.RW2"
fetch "Pentax/K-1"                "IMGP8550.PEF"                              "Pentax_K-1.PEF"
fetch "Pentax/K-1"                "IMGP8551.DNG"                              "Pentax_K-1.DNG"
fetch "Phase One/IQ180"           "230214_9564.IIQ"                           "Phase_One_IQ180.IIQ"
fetch "Samsung/NX1"               "2016-07-23-142101_sam_9364.srw"            "Samsung_NX1.srw"
fetch "Sony/ILCE-7M3"             "_DSC0009.ARW"                              "Sony_ILCE-7M3.ARW"
