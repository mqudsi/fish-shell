BUILD_TYPE := $(shell hash ninja 2>/dev/null >/dev/null \
	&& echo Ninja || echo "Unix Makefiles")
BUILD_FILE := $(shell hash ninja 2>/dev/null >/dev/null \
	&& echo build.ninja || echo Makefile)

build/fish: build/${BUILD_FILE} src/*
	echo $(BUILD_FILE)
	cmake --build build

build:
	mkdir -p ./build

build/${BUILD_FILE}: build
	cd build; cmake .. -G ${BUILD_TYPE} -DCMAKE_EXPORT_COMPILE_COMMANDS=1 -DCMAKE_BUILD_TYPE=RelWithDebInfo

.PHONY: install
install: build/fish
	cmake --build build --target install

.PHONY: test
test: build/fish
	cmake --build build --target test
