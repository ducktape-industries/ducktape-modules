# modules — the wasm32 gates. Every wasm artifact is a build output under
# $(BUILD_TARGET); none is committed.
CARGO ?= cargo
WASM_OPT ?= wasm-opt

# What a program links: abi, guest and store build for wasm32 with nothing else.
PROGRAM_LINKABLE := abi guest store

# The ducktape checkout the probe fixture the founding suite seats is copied
# from (crates/kernel/fixtures, `make kernel-fixtures` there). The probe is a
# dev-dependency only, which cargo cannot build for wasm32 from here.
DUCKTAPE ?= ../core

# Every program, system and app: root members whose program ABI (the guest
# glue, the `alloc`/`call` exports, the `ducktape.*` imports) sits behind their
# `program` feature. Their views link the same crates with the feature off.
# One cargo invocation per program: forge links chat and identity links
# module-registry, and `-p a -p b --features program` in one call would unify
# `program` into the other's link (two `alloc`/`call`).
PROGRAMS := module-registry valset identity chat forge

# Views are wasm32 cdylibs. Chat and Forge ride their own programs;
# Settings rides the registry.
VIEWS := chat-view members-view node-view explorer-view settings-view forge-view

# What a wasm32 view may link. A crate a view links must never reach the
# signing/identity graph (blst does not build for wasm32, and a view has no
# business holding keys). The system crates are here because the system views
# read their contracts: module-registry's signing deps are dev-only, and `-e
# normal` below is what says so. Every program crate is linked with `program`
# off, which is what a plain `-p` build below checks.
VIEW_LINKABLE := ducklink view-wire view-guest design store module-registry valset identity settings-view chat forge
VIEW_FORBIDDEN := blst commonware-cryptography wasm-bindgen js-sys web-sys

# Cargo uses this directory for both workspaces.
BUILD_TARGET := $(abspath $(or $(CARGO_TARGET_DIR),target))
RELEASE := $(BUILD_TARGET)/wasm32-unknown-unknown/release
# The artifacts wasm-modules leaves there (the target dir may hold others).
ARTIFACTS := $(foreach a,$(PROGRAMS) $(VIEWS),$(subst -,_,$(a)).wasm) $(foreach p,$(PROGRAMS),$(subst -,_,$(p)).describe.wasm)

# A wasm artifact must be the same bytes from any checkout on any machine:
# panic locations would otherwise carry this checkout's, cargo's and the
# toolchain's absolute paths.
CARGO_HOME_DIR := $(or $(CARGO_HOME),$(HOME)/.cargo)
SYSROOT := $(shell rustc --print sysroot)
WASM_RUSTFLAGS := --remap-path-prefix=$(CURDIR)=/build --remap-path-prefix=$(CARGO_HOME_DIR)=/cargo --remap-path-prefix=$(SYSROOT)=/rustc
WASM_CARGO := RUSTFLAGS="$(WASM_RUSTFLAGS)" $(CARGO) build --target wasm32-unknown-unknown
WASM_BUILD := $(WASM_CARGO) --release

# A workspace member with a cdylib crate type gets no metadata hash in its
# output name, so `chat` built with `program` (the program, a root) and `chat`
# built without it (a dependency of chat-view, or of forge's program) are one
# unit to cargo's fingerprint and rebuild each other on every invocation.
# Each program therefore builds in its own target dir, where its crate only
# ever appears with `program` on, and its artifact is copied into $(RELEASE)
# beside the views (which build in $(BUILD_TARGET) itself: every view links
# the program crates with `program` off, one unit).
PROGRAM_TARGET = $(BUILD_TARGET)/programs/$1
program_artifact = $(call PROGRAM_TARGET,$1)/wasm32-unknown-unknown/release/$(subst -,_,$1).wasm
program_build = $(WASM_BUILD) --target-dir $(call PROGRAM_TARGET,$1) -p $1 --features program
export CARGO BUILD_TARGET RELEASE WASM_BUILD WASM_OPT

.PHONY: dev wasm-why new-program new-view
.PHONY: program-wasm-check wasm-programs probe-fixture wasm-views view-wasm-check test

# `make dev P=forge` / `V=forge-view` narrow the loop to one artifact.
DEV_PROGRAMS = $(if $(or $P,$V),$P,$(PROGRAMS))
DEV_VIEWS = $(if $(or $P,$V),$V,$(VIEWS))

## the edit loop: rebuilds the programs and views cargo finds stale, gates
## the rebuilt views (ABI), runs the native tests of the crates whose
## test binaries cargo rebuilt; one line per artifact.
dev:
	@tools/dev.sh "$(DEV_PROGRAMS)" "$(DEV_VIEWS)"

## where a view's bytes go: `twiggy top` over a release build that keeps its
## names (`--profile why`: release + `strip = "debuginfo"`, the name section
## kept, its own output dir, so the release artifact is untouched). Install:
## `cargo install twiggy`.
wasm-why:
	@test -n "$V" || { echo "usage: make wasm-why V=<view>"; exit 1; }
	@command -v twiggy >/dev/null || { echo "twiggy is not on PATH: cargo install twiggy"; exit 1; }
	@$(WASM_CARGO) --target-dir $(BUILD_TARGET) --profile why -p $V
	twiggy top -n 25 $(BUILD_TARGET)/wasm32-unknown-unknown/why/$(subst -,_,$V).wasm

## scaffolds crates/app/NAME in chat's shape and registers it (PROGRAMS,
## workspace members and dependencies); `make dev P=NAME` must pass on it.
new-program:
	@test -n "$(NAME)" || { echo "usage: make new-program NAME=<program>"; exit 1; }
	@tools/scaffold.sh program $(NAME)

## scaffolds crates/app/NAME (NAME ends in -view) over the program it names,
## in members-view's shape, and registers it (VIEWS, workspace members).
new-view:
	@test -n "$(NAME)" || { echo "usage: make new-view NAME=<program>-view"; exit 1; }
	@tools/scaffold.sh view $(NAME)

## builds abi and guest for wasm32-unknown-unknown.
program-wasm-check:
	@for crate in $(PROGRAM_LINKABLE); do \
	  $(CARGO) build --target wasm32-unknown-unknown -p $$crate || exit 1; \
	done; \
	echo "abi, guest and store build for wasm32"

## builds every program (with `program` on) into $(RELEASE)/<name>.wasm. The
## founding suite reads the boot set from there.
wasm-programs:
	@mkdir -p $(RELEASE)
	@$(foreach p,$(PROGRAMS),$(call program_build,$(p)) && cp $(call program_artifact,$(p)) $(RELEASE)/ || exit 1;)

## the whole suite; the founding suite runs `make wasm-programs` itself.
test:
	$(CARGO) test --workspace

.PHONY: wasm-describes

# Each program's describe module builds in a target dir of its own, for the
# reason `PROGRAM_TARGET` gives: the crate with `describe` on is another unit.
DESCRIBE_TARGET = $(BUILD_TARGET)/describe/$1
describe_build = $(WASM_BUILD) --target-dir $(call DESCRIBE_TARGET,$1) -p $1 --features describe
describe_artifact = $(call DESCRIBE_TARGET,$1)/wasm32-unknown-unknown/release/$(subst -,_,$1).wasm

## builds every program's describe module (its crate with `describe` on: the
## `alloc`/`describe` exports over its `describe` fn, no imports) into
## $(RELEASE)/<name>.describe.wasm, ABI-checked. qa's pack embeds each as
## the `ducktape.describe` section of its program.
wasm-describes:
	@mkdir -p $(RELEASE)
	@$(foreach p,$(PROGRAMS),$(call describe_build,$(p)) && cp $(call describe_artifact,$(p)) $(RELEASE)/$(subst -,_,$(p)).describe.wasm && python3 tools/check-describe-abi.py $(RELEASE)/$(subst -,_,$(p)).describe.wasm || exit 1;)

.PHONY: wasm-modules wasm-reproducible

## builds every program, its describe module and every view under
## $(RELEASE)/, unpacked. Packing a view and a describe module into its
## program is genesis's job: qa's `kit` pack runs view-pack over these outputs.
wasm-modules: wasm-programs wasm-describes wasm-views
	@cd $(RELEASE) && ls -l $(ARTIFACTS)

## builds every program and view twice, the second time from a fresh target
## directory, and requires the same sha256 for every artifact and no absolute
## path of this checkout or this home inside any of them.
wasm-reproducible:
	$(MAKE) wasm-modules
	@cd $(RELEASE) && sha256sum $(ARTIFACTS) > first.sha256 && cat first.sha256
	$(MAKE) wasm-modules CARGO_TARGET_DIR=$(BUILD_TARGET)/repro
	@cd $(BUILD_TARGET)/repro/wasm32-unknown-unknown/release && sha256sum $(ARTIFACTS) > second.sha256 && cat second.sha256
	@diff $(RELEASE)/first.sha256 $(BUILD_TARGET)/repro/wasm32-unknown-unknown/release/second.sha256 && echo "every program and view rebuilds to the same bytes"
	@for a in $(ARTIFACTS); do f=$(RELEASE)/$$a; \
	  if strings $$f | grep -qE "$(CURDIR)|$(HOME)"; then echo "$$f embeds an absolute path"; strings $$f | grep -E "$(CURDIR)|$(HOME)" | head -3; exit 1; fi; \
	done; echo "no program or view embeds a path of this checkout or home"

## refreshes the probe fixture the founding suite seats as the authority,
## from the ducktape checkout at $(DUCKTAPE).
probe-fixture:
	cp $(DUCKTAPE)/crates/kernel/fixtures/wasm/fixture_probe.wasm crates/system/module-registry/tests/

## builds every view for wasm32 under $(RELEASE)/, optimized (the bytes
## before wasm-opt stay beside it as <name>.wasm.unoptimized), ABI-checked
## and its size printed: `name  bytes`.
wasm-views:
	@$(foreach v,$(VIEWS),$(WASM_BUILD) --target-dir $(BUILD_TARGET) -p $(v) && tools/view-gate.sh $(v) $(RELEASE)/$(subst -,_,$(v)).wasm || exit 1;)

## builds every VIEW_LINKABLE crate for wasm32-unknown-unknown, plus the
## exported view probe of view-guest, then fails if the normal wasm32 dependency
## tree of any of them names a VIEW_FORBIDDEN crate.
view-wasm-check:
	@for crate in $(VIEW_LINKABLE); do \
	  $(CARGO) build --target wasm32-unknown-unknown -p $$crate || exit 1; \
	done; \
	$(CARGO) build --target wasm32-unknown-unknown -p view-guest --example exported_view --example media_probe || exit 1; \
	reached=""; \
	for crate in $(VIEW_LINKABLE); do \
	  tree=$$($(CARGO) tree --target wasm32-unknown-unknown -e normal -p $$crate --prefix none) || exit 1; \
	  for dep in $(VIEW_FORBIDDEN); do \
	    if echo "$$tree" | grep -q "^$$dep v"; then reached="$$reached $$crate->$$dep"; fi; \
	  done; \
	done; \
	if [ -z "$$reached" ]; then \
	  echo "every view-linkable crate builds for wasm32 and stays off the signing/identity and JavaScript graphs"; \
	else \
	  echo "view-linkable crates reach a forbidden dependency:$$reached"; \
	  exit 1; \
	fi
