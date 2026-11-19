# Builds every plugin under plugins/ into dist/<name>/, ready to copy to /plugins/ on the SD card.
RISCV ?= $(HOME)/.platformio/tools/toolchain-riscv32-esp/bin/riscv32-esp-elf-
SDK = dessen/sdk
APPFLAGS = -march=rv32im -mabi=ilp32 -Os -ffreestanding -nostdlib -mno-relax \
           -fno-tree-loop-distribute-patterns -Wall -I$(SDK) -T $(SDK)/app.ld -Wl,--no-warn-rwx-segments
PLUGINS = $(notdir $(wildcard plugins/*))

all: $(PLUGINS:%=dist/%/app.bin)

dist/%/app.bin: plugins/%/*.c plugins/%/*.h plugins/%/*.json $(SDK)/crt0.S $(SDK)/rt.c $(SDK)/app.ld
	@mkdir -p build dist/$*
	$(RISCV)gcc $(APPFLAGS) $(SDK)/crt0.S $(SDK)/rt.c plugins/$*/$*.c -o build/$*.elf
	$(RISCV)objcopy -O binary build/$*.elf $@
	cp plugins/$*/*.json plugins/$*/README.md dist/$*/

clean:
	rm -rf build dist

.PHONY: all clean
