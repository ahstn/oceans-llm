#!/usr/bin/env bash
# Loudness curve, octave balance and pictures for an audio file (WAV or the rendered MP4).
# Usage: .scratch/audio/analyze.sh <file> [name]
set -euo pipefail
IN="$1"
NAME="${2:-score}"
OUT="$(dirname "$0")"

echo "--- summary"
ffmpeg -hide_banner -nostats -i "$IN" -af ebur128=peak=true -f null - 2>&1 | grep -E "^ +(I|LRA|Peak):" || true

echo "--- octave balance over the flight (2.8-10.8s), dB relative to the 1 kHz band"
ref=""
for f in 31 63 125 250 500 1000 2000 4000 8000 16000; do
  lvl=$(ffmpeg -hide_banner -nostats -ss 2.8 -t 8 -i "$IN" -af "pan=mono|c0=0.5*c0+0.5*c1,bandpass=f=$f:width_type=o:w=1,bandpass=f=$f:width_type=o:w=1,astats=metadata=0:measure_overall=RMS_level:measure_perchannel=none" -f null - 2>&1 | grep -m1 "RMS level" | awk '{print $NF}')
  echo "$f $lvl"
done | awk '{lv[NR]=$2; f[NR]=$1; if ($1==1000) r=$2} END {for (i=1;i<=NR;i++) printf "%6s Hz %+6.1f\n", f[i], lv[i]-r}'

echo "--- momentary loudness (400 ms window ending at t)"
ffmpeg -hide_banner -nostats -v verbose -i "$IN" -af ebur128=framelog=verbose -f null - 2>&1 |
  grep -E "Parsed_ebur128.* t: " |
  sed -E 's/.* t: *([0-9.]+).* M: *(-?[0-9.]+|-inf).*/\1 \2/' |
  awk '{t=$1; m=$2; if (int(t*10+0.5)%2==0) {n=(m+36>0?int(m+36):0); bar=""; for(i=0;i<n;i++) bar=bar "#"; printf "%5.1f %6.1f %s\n", t, m, bar}}'

ffmpeg -hide_banner -loglevel error -y -i "$IN" -lavfi "showspectrumpic=s=1500x540:legend=1:fscale=log:scale=log:color=intensity:start=30:stop=20000:drange=90" "$OUT/$NAME-spec.png"
ffmpeg -hide_banner -loglevel error -y -i "$IN" -lavfi "aformat=channel_layouts=mono,showwavespic=s=1500x260:colors=#0b83ad:scale=lin:draw=full" "$OUT/$NAME-wave.png"
echo "--- wrote $OUT/$NAME-spec.png, $OUT/$NAME-wave.png"
