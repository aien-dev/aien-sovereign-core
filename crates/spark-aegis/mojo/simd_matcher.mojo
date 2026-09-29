# spark-aegis Mojo SIMD acceleration module
# Vector signature matching and byte scanning

@export("mojo_simd_version")
def mojo_simd_version() abi("C") -> Int32:
    return 1

@export("mojo_simd_scan_byte")
def mojo_simd_scan_byte(data: Pointer[UInt8, MutUntrackedOrigin], length: Int, target: UInt8) abi("C") -> Int:
    comptime width = 16
    var i: Int = 0
    var target_vec = SIMD[DType.uint8, width](target)

    while i + width <= length:
        var chunk = data.unsafe_load[width=width](i)
        var cmp = chunk.eq(target_vec)
        if cmp.reduce_or():
            for offset in range(width):
                if data.unsafe_load(i + offset) == target:
                    return i + offset
        i += width

    while i < length:
        if data.unsafe_load(i) == target:
            return i
        i += 1

    return -1

@export("mojo_simd_count_matches")
def mojo_simd_count_matches(data: Pointer[UInt8, MutUntrackedOrigin], length: Int, target: UInt8) abi("C") -> Int:
    comptime width = 16
    var i: Int = 0
    var count: Int = 0
    var target_vec = SIMD[DType.uint8, width](target)

    while i + width <= length:
        var chunk = data.unsafe_load[width=width](i)
        var cmp = chunk.eq(target_vec)
        if cmp.reduce_or():
            for offset in range(width):
                if data.unsafe_load(i + offset) == target:
                    count += 1
        i += width

    while i < length:
        if data.unsafe_load(i) == target:
            count += 1
        i += 1

    return count

@export("mojo_simd_find_pattern")
def mojo_simd_find_pattern(data: Pointer[UInt8, MutUntrackedOrigin], data_len: Int, pattern: Pointer[UInt8, MutUntrackedOrigin], pattern_len: Int) abi("C") -> Int:
    if pattern_len <= 0 or data_len < pattern_len:
        return -1

    var first_byte = pattern.unsafe_load(0)
    var cursor: Int = 0

    while cursor <= data_len - pattern_len:
        var remaining = data_len - cursor
        var relative_match = mojo_simd_scan_byte(data.unsafe_offset(cursor), remaining, first_byte)
        if relative_match < 0:
            return -1

        var cand = cursor + relative_match
        if cand > data_len - pattern_len:
            return -1

        var full_match = True
        for j in range(1, pattern_len):
            if data.unsafe_load(cand + j) != pattern.unsafe_load(j):
                full_match = False
                break

        if full_match:
            return cand

        cursor = cand + 1

    return -1
