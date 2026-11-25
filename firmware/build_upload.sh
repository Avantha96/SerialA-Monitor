#!/usr/bin/env bash
# Build an Arduino sketch for the Nano (ATmega328P, 16 MHz) and optionally upload it,
# using the toolchain the Arduino IDE / Boards Manager installed under %LOCALAPPDATA%\Arduino15.
#
#   ./build_upload.sh hs_stream            # build only -> build/hs_stream.hex
#   ./build_upload.sh hs_stream COM9       # build and upload
#
# Upload tries the new bootloader (115200) first, then the old one (57600).
set -euo pipefail

SKETCH="${1:?usage: build_upload.sh <sketch-folder> [COMx]}"
PORT="${2:-}"
HERE="$(cd "$(dirname "$0")" && pwd)"
SRC="$HERE/$SKETCH/$SKETCH.ino"
OUT="$HERE/build/$SKETCH"

A15="${LOCALAPPDATA:-$HOME/AppData/Local}/Arduino15/packages/arduino"
AVR="$(ls -d "$A15"/hardware/avr/* | sort -V | tail -1)"
GCC="$(ls -d "$A15"/tools/avr-gcc/*/bin | sort -V | tail -1)"
DUDE_DIR="$(ls -d "$A15"/tools/avrdude/* | sort -V | tail -1)"
DUDE="$DUDE_DIR/bin/avrdude"
DUDE_CONF="$(ls "$DUDE_DIR"/etc/avrdude.conf 2>/dev/null || ls "$DUDE_DIR"/bin/avrdude.conf)"

CORE="$AVR/cores/arduino"
VARIANT="$AVR/variants/eightanaloginputs"
MCU=atmega328p
DEFS="-DF_CPU=16000000L -DARDUINO=10819 -DARDUINO_AVR_NANO -DARDUINO_ARCH_AVR"
INC=(-I"$CORE" -I"$VARIANT")
COMMON="-c -g -Os -w -ffunction-sections -fdata-sections -MMD -flto -mmcu=$MCU $DEFS"
CFLAGS="$COMMON -std=gnu11 -fno-fat-lto-objects"
CXXFLAGS="$COMMON -std=gnu++11 -fpermissive -fno-exceptions -fno-threadsafe-statics -Wno-error=narrowing"
SFLAGS="-c -g -x assembler-with-cpp -flto -MMD -mmcu=$MCU $DEFS"

[ -f "$SRC" ] || { echo "sketch not found: $SRC"; exit 1; }
rm -rf "$OUT" && mkdir -p "$OUT/core"
echo "core:    $AVR"
echo "gcc:     $GCC"

# Arduino core -> core.a
for f in "$CORE"/*.c;   do "$GCC/avr-gcc" $CFLAGS   "${INC[@]}" "$f" -o "$OUT/core/$(basename "$f").o"; done
for f in "$CORE"/*.cpp; do "$GCC/avr-g++" $CXXFLAGS "${INC[@]}" "$f" -o "$OUT/core/$(basename "$f").o"; done
for f in "$CORE"/*.S;   do "$GCC/avr-gcc" $SFLAGS   "${INC[@]}" "$f" -o "$OUT/core/$(basename "$f").o"; done
"$GCC/avr-gcc-ar" rcs "$OUT/core.a" "$OUT"/core/*.o

# Sketch (.ino is C++ with Arduino.h included first)
{ echo '#include <Arduino.h>'; echo "#line 1 \"$SKETCH.ino\""; cat "$SRC"; } > "$OUT/$SKETCH.cpp"
"$GCC/avr-g++" $CXXFLAGS "${INC[@]}" "$OUT/$SKETCH.cpp" -o "$OUT/$SKETCH.o"

# Link -> .hex
"$GCC/avr-gcc" -w -Os -g -flto -fuse-linker-plugin -Wl,--gc-sections -mmcu=$MCU \
  -o "$OUT/$SKETCH.elf" "$OUT/$SKETCH.o" "$OUT/core.a" -L"$OUT" -lm
"$GCC/avr-objcopy" -O ihex -R .eeprom "$OUT/$SKETCH.elf" "$HERE/build/$SKETCH.hex"
"$GCC/avr-size" -C --mcu=$MCU "$OUT/$SKETCH.elf" | grep -E "Program|Data"
echo "hex:     $HERE/build/$SKETCH.hex"

[ -n "$PORT" ] || exit 0
for rate in 115200 57600; do
  echo "upload:  $PORT @ $rate"
  # Run from the build folder with a bare file name: avrdude is a native Windows program
  # and can't open MSYS-style paths (/e/...) inside the -U argument.
  # timeout: if the board is streaming at another baud rate, a wrong-rate attempt can wait
  # forever for the line to go quiet.
  if (cd "$HERE/build" && timeout 30 "$DUDE" -C "$DUDE_CONF" -p $MCU -c arduino -P "$PORT" -b $rate -D -q \
       -U "flash:w:$SKETCH.hex:i"); then
    echo "upload OK"
    exit 0
  fi
done
echo "upload FAILED (is the port open in another program?)"
exit 1
