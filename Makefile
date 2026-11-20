# Builds every plugin under plugins/ into dist/<name>/, ready to copy to /plugins/ on the SD card.
RISCV ?= $(HOME)/.platformio/tools/toolchain-riscv32-esp/bin/riscv32-esp-elf-
SDK = dessen/sdk
APPFLAGS = -march=rv32im -mabi=ilp32 -Os -ffreestanding -nostdlib -mno-relax \
           -fno-tree-loop-distribute-patterns -Wall -I$(SDK) -T $(SDK)/app.ld -Wl,--no-warn-rwx-segments
comma = ,
PLUGINS = $(notdir $(wildcard plugins/*))
# App memory per plugin, when it needs more than the SDK's 64 KB (PSRAM boards allow up to 4 MB).
APP_MEM_ticket = 1572864

all: $(PLUGINS:%=dist/%/app.bin)

dist/%/app.bin: plugins/%/*.c plugins/%/*.h plugins/%/*.json $(SDK)/crt0.S $(SDK)/rt.c $(SDK)/app.ld
	@mkdir -p build dist/$*
	$(RISCV)gcc $(APPFLAGS) $(if $(APP_MEM_$*),-Wl$(comma)--defsym=APP_MEM=$(APP_MEM_$*)) $(SDK)/crt0.S $(SDK)/rt.c plugins/$*/$*.c -o build/$*.elf
	$(RISCV)objcopy -O binary build/$*.elf $@
	cp plugins/$*/*.json plugins/$*/README.md dist/$*/
	for d in fonts stamps; do if [ -d plugins/$*/$$d ]; then rm -rf dist/$*/$$d && cp -R plugins/$*/$$d dist/$*/; fi; done

clean:
	rm -rf build dist

.PHONY: all clean
