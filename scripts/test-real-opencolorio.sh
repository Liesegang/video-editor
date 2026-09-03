#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPOSITORY_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"

# PyO3 and the embedded runtime must use the same pinned interpreter in this
# standalone gate, just as they do in the repository-wide quality gate.
# shellcheck source=scripts/managed-python-env.sh
source "${SCRIPT_DIR}/managed-python-env.sh"

readonly OCIO_VERSION="2.5.2"
readonly OCIO_SOURCE_SHA256="722601e01b78b7a12da4829cb450674935f404b0e508f3f20046fa77570e3272"
readonly OCIO_SOURCE_URL="https://github.com/AcademySoftwareFoundation/OpenColorIO/archive/refs/tags/v${OCIO_VERSION}.tar.gz"

REAL_OCIO_CACHE_DIR="${RUVIE_REAL_OCIO_CACHE_DIR:-${REPOSITORY_ROOT}/target/real-opencolorio-cache}"
REAL_OCIO_TARGET_DIR="${RUVIE_REAL_OCIO_TARGET_DIR:-${REPOSITORY_ROOT}/target/real-opencolorio-cargo}"
OCIO_ARCHIVE="${REAL_OCIO_CACHE_DIR}/downloads/OpenColorIO-${OCIO_VERSION}.tar.gz"
OCIO_SOURCE_ROOT="${REAL_OCIO_CACHE_DIR}/source"
OCIO_SOURCE_DIR="${OCIO_SOURCE_ROOT}/OpenColorIO-${OCIO_VERSION}"
OCIO_BUILD_DIR="${REAL_OCIO_CACHE_DIR}/build/OpenColorIO-${OCIO_VERSION}"
OCIO_INSTALL_DIR="${REAL_OCIO_CACHE_DIR}/install/OpenColorIO-${OCIO_VERSION}"
OCIO_INSTALL_MARKER="${OCIO_INSTALL_DIR}/.ruvie-real-ocio"

die() {
    printf '[real-ocio] error: %s\n' "$*" >&2
    exit 1
}

sha256_file() {
    local path="$1"
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum -- "$path" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 -- "$path" | awk '{print $1}'
    else
        die "sha256sum or shasum is required"
    fi
}

assert_expected_archive() {
    local path="$1"
    local actual
    actual="$(sha256_file "$path")"
    [[ "$actual" == "$OCIO_SOURCE_SHA256" ]] ||
        die "OpenColorIO archive checksum mismatch: expected ${OCIO_SOURCE_SHA256}, got ${actual}"
}

assert_safe_archive() {
    local path="$1"
    local member
    while IFS= read -r member; do
        [[ -n "$member" ]] || continue
        [[ "$member" != /* ]] || die "archive contains an absolute path: ${member}"
        case "/${member}/" in
            */../*) die "archive contains a parent traversal: ${member}" ;;
        esac
        [[ "$member" == "OpenColorIO-${OCIO_VERSION}"/* || "$member" == "OpenColorIO-${OCIO_VERSION}/" ]] ||
            die "archive member is outside the expected root: ${member}"
    done < <(tar -tzf "$path")
}

assert_exact_rust_dependency() {
    grep -Fq 'ocio-rs = { version = "=0.2.1"' "${REPOSITORY_ROOT}/color-management/Cargo.toml" ||
        die "color-management must pin ocio-rs exactly to 0.2.1"
}

run_self_test() {
    [[ "$OCIO_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "invalid pinned OCIO version"
    [[ "$OCIO_SOURCE_SHA256" =~ ^[0-9a-f]{64}$ ]] || die "invalid pinned source checksum"
    [[ "$OCIO_SOURCE_URL" == https://github.com/AcademySoftwareFoundation/OpenColorIO/* ]] ||
        die "source URL is not the official OpenColorIO repository"
    assert_exact_rust_dependency
    grep -Fq './scripts/test-real-opencolorio.sh' "${REPOSITORY_ROOT}/.github/workflows/quality.yml" ||
        die "quality workflow does not run the real OpenColorIO gate"
    [[ -f "${REPOSITORY_ROOT}/color-management/tests/real_ocio_non_identity.rs" ]] ||
        die "non-identity numeric test source is missing"
    grep -Fq 'production_named_ocio_preview_chains_view_output_to_bound_srgb_surface' \
        "${REPOSITORY_ROOT}/library/src/core/rendering/managed_color_runtime_tests.rs" ||
        die "production OCIO surface-authority test source is missing"
    grep -Eq '^[[:space:]]*run_real_non_identity_test$' "${BASH_SOURCE[0]}" ||
        die "real OpenColorIO gate does not invoke the non-identity numeric test"
    grep -Eq '^[[:space:]]*run_real_production_surface_test$' "${BASH_SOURCE[0]}" ||
        die "real OpenColorIO gate does not invoke the production surface-authority test"
    printf '[real-ocio] self-test passed; no runtime build was executed\n'
}

download_source() {
    local download_tmp
    mkdir -p -- "$(dirname -- "$OCIO_ARCHIVE")"
    if [[ -f "$OCIO_ARCHIVE" ]]; then
        if [[ "$(sha256_file "$OCIO_ARCHIVE")" == "$OCIO_SOURCE_SHA256" ]]; then
            printf '[real-ocio] using checksum-verified cached source archive\n'
            return
        fi
        printf '[real-ocio] cached archive is invalid; replacing it from the pinned URL\n' >&2
    fi

    download_tmp="$(mktemp "${OCIO_ARCHIVE}.download.XXXXXX")"
    trap 'rm -f -- "${download_tmp:-}"' RETURN
    curl --fail --location --show-error --silent --retry 3 --retry-all-errors \
        --output "$download_tmp" "$OCIO_SOURCE_URL"
    assert_expected_archive "$download_tmp"
    mv -f -- "$download_tmp" "$OCIO_ARCHIVE"
    trap - RETURN
}

extract_source() {
    local extract_tmp
    if [[ -f "${OCIO_SOURCE_DIR}/CMakeLists.txt" ]]; then
        return
    fi
    [[ ! -e "$OCIO_SOURCE_DIR" ]] ||
        die "incomplete cached source directory exists: ${OCIO_SOURCE_DIR}"

    mkdir -p -- "$OCIO_SOURCE_ROOT"
    extract_tmp="$(mktemp -d "${OCIO_SOURCE_ROOT}/.extract.XXXXXX")"
    trap 'rm -rf -- "${extract_tmp:-}"' RETURN
    assert_safe_archive "$OCIO_ARCHIVE"
    tar -xzf "$OCIO_ARCHIVE" -C "$extract_tmp"
    [[ -f "${extract_tmp}/OpenColorIO-${OCIO_VERSION}/CMakeLists.txt" ]] ||
        die "archive does not contain the expected OpenColorIO source root"
    mv -- "${extract_tmp}/OpenColorIO-${OCIO_VERSION}" "$OCIO_SOURCE_DIR"
    rmdir -- "$extract_tmp"
    trap - RETURN
}

available_build_jobs() {
    if [[ -n "${RUVIE_REAL_OCIO_BUILD_JOBS:-}" ]]; then
        printf '%s\n' "$RUVIE_REAL_OCIO_BUILD_JOBS"
    elif command -v nproc >/dev/null 2>&1; then
        nproc
    elif command -v sysctl >/dev/null 2>&1; then
        sysctl -n hw.logicalcpu
    else
        printf '2\n'
    fi
}

install_outputs_exist() {
    [[ -f "${OCIO_INSTALL_DIR}/include/OpenColorIO/OpenColorIO.h" ]] || return 1
    find "${OCIO_INSTALL_DIR}" -type f \( -name 'libOpenColorIO.so*' -o -name 'libOpenColorIO.*.dylib' \) \
        -print -quit | grep -q .
}

install_is_exact() {
    [[ -f "$OCIO_INSTALL_MARKER" ]] || return 1
    grep -Fxq "version=${OCIO_VERSION}" "$OCIO_INSTALL_MARKER" || return 1
    grep -Fxq "source_sha256=${OCIO_SOURCE_SHA256}" "$OCIO_INSTALL_MARKER" || return 1
    install_outputs_exist
}

build_exact_install() {
    local jobs
    if install_is_exact; then
        printf '[real-ocio] using cached exact OpenColorIO install\n'
        return
    fi
    [[ ! -e "$OCIO_INSTALL_DIR" ]] ||
        die "incomplete or mismatched cached install exists: ${OCIO_INSTALL_DIR}"

    jobs="$(available_build_jobs)"
    [[ "$jobs" =~ ^[1-9][0-9]*$ ]] || die "RUVIE_REAL_OCIO_BUILD_JOBS must be a positive integer"
    mkdir -p -- "$OCIO_BUILD_DIR" "$(dirname -- "$OCIO_INSTALL_DIR")"

    cmake -S "$OCIO_SOURCE_DIR" -B "$OCIO_BUILD_DIR" -G Ninja \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_INSTALL_PREFIX="$OCIO_INSTALL_DIR" \
        -DBUILD_SHARED_LIBS=ON \
        -DOCIO_BUILD_APPS=OFF \
        -DOCIO_BUILD_DOCS=OFF \
        -DOCIO_BUILD_GPU_TESTS=OFF \
        -DOCIO_BUILD_JAVA=OFF \
        -DOCIO_BUILD_NUKE=OFF \
        -DOCIO_BUILD_OPENFX=OFF \
        -DOCIO_BUILD_PYTHON=OFF \
        -DOCIO_BUILD_TESTS=OFF \
        -DOCIO_INSTALL_EXT_PACKAGES=ALL
    cmake --build "$OCIO_BUILD_DIR" --parallel "$jobs"
    cmake --install "$OCIO_BUILD_DIR"

    install_outputs_exist || die "OpenColorIO install did not produce the expected shared library"
    printf 'version=%s\nsource_sha256=%s\n' "$OCIO_VERSION" "$OCIO_SOURCE_SHA256" >"$OCIO_INSTALL_MARKER"
}

run_real_non_identity_test() {
    printf '[real-ocio] verifying a non-identity transform against an independent oracle\n'
    cargo test -p ruvie-color-management --test real_ocio_non_identity \
        --features opencolorio --locked
}

run_real_production_surface_test() {
    printf '[real-ocio] verifying Project display/view output is converted to the exact native sRGB surface\n'
    cargo test -p library --lib --features opencolorio --locked \
        production_named_ocio_preview_chains_view_output_to_bound_srgb_surface
}

run_real_runtime_gate() {
    local runtime_library_path
    runtime_library_path="${OCIO_INSTALL_DIR}/lib:${OCIO_INSTALL_DIR}/lib64"
    export OCIO_RS_ENABLE_REAL=1
    export OCIO_INSTALL_DIR
    export OCIO_RS_LINK=dynamic
    export CARGO_TARGET_DIR="$REAL_OCIO_TARGET_DIR"
    export RUVIE_REQUIRE_REAL_OCIO=1
    export LD_LIBRARY_PATH="${runtime_library_path}${LD_LIBRARY_PATH:+:${LD_LIBRARY_PATH}}"
    export DYLD_LIBRARY_PATH="${runtime_library_path}${DYLD_LIBRARY_PATH:+:${DYLD_LIBRARY_PATH}}"

    cd "$REPOSITORY_ROOT"
    printf '[real-ocio] asserting non-stub runtime before the OCIO test suite\n'
    cargo run -p ruvie-color-management --example assert_real_opencolorio \
        --features opencolorio --locked

    run_real_non_identity_test
    run_real_production_surface_test

    printf '[real-ocio] running color-management tests against the verified runtime\n'
    cargo test -p ruvie-color-management --features opencolorio --locked

    printf '[real-ocio] checking the app with the verified runtime\n'
    cargo check -p app --features opencolorio --locked
}

case "${1:-}" in
    --self-test)
        [[ "$#" -eq 1 ]] || die "--self-test accepts no additional arguments"
        run_self_test
        ;;
    --help|-h)
        printf 'Usage: %s [--self-test]\n' "$0"
        ;;
    "")
        assert_exact_rust_dependency
        command -v cmake >/dev/null 2>&1 || die "cmake is required"
        command -v curl >/dev/null 2>&1 || die "curl is required"
        command -v ninja >/dev/null 2>&1 || die "ninja is required"
        command -v tar >/dev/null 2>&1 || die "tar is required"
        download_source
        assert_expected_archive "$OCIO_ARCHIVE"
        extract_source
        build_exact_install
        run_real_runtime_gate
        ;;
    *)
        die "unknown argument: $1"
        ;;
esac
