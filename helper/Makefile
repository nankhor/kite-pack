# The Kite helper.
#
#   make host   build/host/kite-helper, for this computer (tests run it as a stand-in PS5)
#   make ps5    kite-helper.elf, with the PS5 payload SDK (PS5_PAYLOAD_SDK set; CI does this)

VERSION := 0.1.0
AVA1 := third_party/ava1/c
MONO := $(AVA1)/third_party/monocypher
B3 := $(AVA1)/third_party/blake3

AVA1_SRCS := $(filter-out $(AVA1)/platform_ps5.c $(AVA1)/platform_posix.c $(AVA1)/ava1_chacha_avx2.c,$(wildcard $(AVA1)/*.c)) \
             $(AVA1)/gen/ava1_gen.c
DEP_SRCS := $(MONO)/monocypher.c $(B3)/blake3.c $(B3)/blake3_dispatch.c $(B3)/blake3_portable.c
OUR_SRCS := src/kite_helper.c
HEADERS := $(wildcard src/*.h) $(wildcard $(AVA1)/*.h) $(AVA1)/gen/ava1_gen.h

INC := -Isrc -I$(AVA1) -I$(AVA1)/gen -I$(MONO) -I$(B3)
B3_DEFS := -DBLAKE3_NO_SSE2 -DBLAKE3_NO_SSE41 -DBLAKE3_NO_AVX2 -DBLAKE3_NO_AVX512 -DBLAKE3_USE_NEON=0
DEFS := -DKITE_VERSION='"$(VERSION)"' $(B3_DEFS)

.PHONY: host ps5 clean

# ---- this computer ----------------------------------------------------------------------------

HOST_CC ?= cc
HOST_OUT := build/host/kite-helper
ARCH := $(shell uname -m)

host: $(HOST_OUT)

# The AVX2 ChaCha20 is its own unit with -mavx2 (x86-64 only; the AEAD checks CPUID first).
build/host/chacha_avx2.o: $(AVA1)/ava1_chacha_avx2.c $(HEADERS)
	@mkdir -p $(dir $@)
	$(HOST_CC) -O2 -std=gnu11 -w $(if $(filter x86_64 amd64,$(ARCH)),-mavx2) $(INC) -c -o $@ $<

$(HOST_OUT): $(OUR_SRCS) src/notify_host.c $(AVA1_SRCS) $(AVA1)/platform_posix.c $(DEP_SRCS) $(HEADERS) build/host/chacha_avx2.o Makefile
	@mkdir -p $(dir $@)
	$(HOST_CC) -O2 -std=gnu11 -pthread -w $(INC) $(DEFS) -DKITE_PLATFORM='"host"' -o $@ \
		$(OUR_SRCS) src/notify_host.c $(AVA1_SRCS) $(AVA1)/platform_posix.c $(DEP_SRCS) build/host/chacha_avx2.o

# ---- the PS5 ----------------------------------------------------------------------------------

ps5: kite-helper.elf

ifdef PS5_PAYLOAD_SDK
include $(PS5_PAYLOAD_SDK)/toolchain/prospero.mk

build/ps5/chacha_avx2.o: $(AVA1)/ava1_chacha_avx2.c $(HEADERS)
	@mkdir -p $(dir $@)
	$(CC) -O2 -std=gnu11 -w -mavx2 $(INC) -c -o $@ $<

kite-helper.elf: $(OUR_SRCS) src/notify_ps5.c $(AVA1_SRCS) $(AVA1)/platform_ps5.c $(DEP_SRCS) $(HEADERS) build/ps5/chacha_avx2.o Makefile
	$(CC) -O2 -std=gnu11 -pthread -w $(INC) $(DEFS) -DKITE_PLATFORM='"ps5"' -o $@ \
		$(OUR_SRCS) src/notify_ps5.c $(AVA1_SRCS) $(AVA1)/platform_ps5.c $(DEP_SRCS) build/ps5/chacha_avx2.o
	$(STRIP) $@
else
kite-helper.elf:
	$(error PS5_PAYLOAD_SDK is not set: the PS5 build needs the PS5 payload SDK)
endif

clean:
	rm -rf build kite-helper.elf
