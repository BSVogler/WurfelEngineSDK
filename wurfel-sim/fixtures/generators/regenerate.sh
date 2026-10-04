#!/bin/bash
# Regenerates the *.txt fixtures by compiling the REAL Java generators (unmodified) against the tiny
# stubs in stubs/ and running Harness. The Rust tests compare the Rust generators with these files.
#   needs: a JDK (javac), the WurfelEngineSDK repo (this one) and the Caveland repo next to it.
set -e
cd "$(dirname "$0")"
SDK="$(cd ../../.. && pwd)"
CAVELAND="${CAVELAND_DIR:-$SDK/../Caveland}"
OUT="$(mktemp -d)"

# The stub ids must match the real enum.
for pair in ENTRY:16 INDESTRUCTIBLEOBSTACLE:17 SULFUR:42 IRONORE:43 COAL:44; do
  grep -q "${pair%%:*}((byte) ${pair##*:}," "$CAVELAND/src/main/java/com/bombinggames/caveland/game/CavelandBlocks.java" \
    || { echo "stub id mismatch for $pair"; exit 1; }
done
grep -q "OBJECTTYPESNUM = 124" "$SDK/core/src/main/java/com/bombinggames/wurfelengine/core/map/rendering/RenderCell.java" \
  || { echo "OBJECTTYPESNUM changed"; exit 1; }

G="$SDK/core/src/main/java/com/bombinggames/wurfelengine/core/map"
javac -nowarn -d "$OUT" \
  $(find stubs -name '*.java') Harness.java \
  "$G/Generator.java" "$G/Generators/AirGenerator.java" "$G/Generators/IslandGenerator.java" \
  "$G/Generators/BlockTestGenerator.java" "$G/Generators/FullMapGenerator.java" \
  "$SDK/demogame/src/main/java/com/bombinggames/weaponofchoice/ArenaGenerator.java" \
  "$CAVELAND/src/main/java/com/bombinggames/caveland/game/ChunkGenerator.java"
java -cp "$OUT" Harness "$PWD"
rm -rf "$OUT"
wc -c *.txt
