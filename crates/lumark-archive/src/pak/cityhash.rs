//! CityHash64（Google CityHash v1.0.2）与 UE5 `FPackageId` 计算。
//!
//! UE5 IoStore 的包 ID = 包名（小写 UTF-16LE）的 CityHash64。参考：
//! Google cityhash 原始算法；CUE4Parse `UE4/IO/Objects/FPackageId.cs` 的
//! 行为说明（Apache-2.0），本文件按算法独立实现。

const K0: u64 = 0xc3a5c85c97cb3127;
const K1: u64 = 0xb492b66fbe98f273;
const K2: u64 = 0x9ae16a3b2f90404f;

/// 计算 UE5 包名对应的 `FPackageId`（小写 UTF-16LE + CityHash64）。
pub fn package_id(name: &str) -> u64 {
    let mut bytes = Vec::with_capacity(name.len() * 2);
    for character in name.chars() {
        let lower = character.to_ascii_lowercase();
        let mut buffer = [0u16; 2];
        for unit in lower.encode_utf16(&mut buffer).iter() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
    }
    city_hash64(&bytes)
}

/// CityHash64。
pub fn city_hash64(data: &[u8]) -> u64 {
    let len = data.len();
    if len <= 16 {
        return hash_len_0_to_16(data);
    }
    if len <= 32 {
        return hash_len_17_to_32(data);
    }
    if len <= 64 {
        return hash_len_33_to_64(data);
    }
    let mut x = fetch64(data, len - 40);
    let mut y = fetch64(data, len - 16).wrapping_add(fetch64(data, len - 56));
    let mut z = hash_len_16(
        fetch64(data, len - 48).wrapping_add(len as u64),
        fetch64(data, len - 24),
    );
    let mut v = weak_hash_len_32_with_seeds(data, len - 64, len as u64, z);
    let mut w = weak_hash_len_32_with_seeds(data, len - 32, y.wrapping_add(K1), x);
    x = x.wrapping_mul(K1).wrapping_add(fetch64(data, 0));
    let mut remaining = (len - 1) & !63;
    let mut offset = 0usize;
    loop {
        x = rotate(
            x.wrapping_add(y)
                .wrapping_add(v.0)
                .wrapping_add(fetch64(data, offset + 8)),
            37,
        )
        .wrapping_mul(K1);
        y = rotate(
            y.wrapping_add(v.1).wrapping_add(fetch64(data, offset + 48)),
            42,
        )
        .wrapping_mul(K1);
        x ^= w.1;
        y = y.wrapping_add(v.0).wrapping_add(fetch64(data, offset + 40));
        z = rotate(z.wrapping_add(w.0), 33).wrapping_mul(K1);
        v = weak_hash_len_32_with_seeds(data, offset, v.1.wrapping_mul(K1), x.wrapping_add(w.0));
        w = weak_hash_len_32_with_seeds(
            data,
            offset + 32,
            z.wrapping_add(w.1),
            y.wrapping_add(fetch64(data, offset + 16)),
        );
        std::mem::swap(&mut z, &mut x);
        offset += 64;
        remaining = remaining.saturating_sub(64);
        if remaining == 0 {
            break;
        }
    }
    hash_len_16(
        hash_len_16(v.0, w.0)
            .wrapping_add(shift_mix(y).wrapping_mul(K1))
            .wrapping_add(z),
        hash_len_16(v.1, w.1).wrapping_add(x),
    )
}

fn hash_len_0_to_16(data: &[u8]) -> u64 {
    let len = data.len();
    if len >= 8 {
        let mul = K2.wrapping_add((len as u64).wrapping_mul(2));
        let a = fetch64(data, 0).wrapping_add(K2);
        let b = fetch64(data, len - 8);
        let c = rotate(b, 37).wrapping_mul(mul).wrapping_add(a);
        let d = rotate(a, 25).wrapping_add(b).wrapping_mul(mul);
        return hash_len_16_with_mul(c, d, mul);
    }
    if len >= 4 {
        let mul = K2.wrapping_add((len as u64).wrapping_mul(2));
        let a = fetch32(data, 0);
        return hash_len_16_with_mul(
            (len as u64).wrapping_add((a as u64) << 3),
            fetch32(data, len - 4) as u64,
            mul,
        );
    }
    if len > 0 {
        let a = data[0] as u32;
        let b = data[len >> 1] as u32;
        let c = data[len - 1] as u32;
        let y = a.wrapping_add(b << 8);
        let z = (len as u32).wrapping_add(c << 2);
        return shift_mix((y as u64).wrapping_mul(K2) ^ (z as u64).wrapping_mul(K0))
            .wrapping_mul(K2);
    }
    K2
}

fn hash_len_17_to_32(data: &[u8]) -> u64 {
    let len = data.len();
    let mul = K2.wrapping_add((len as u64).wrapping_mul(2));
    let a = fetch64(data, 0).wrapping_mul(K1);
    let b = fetch64(data, 8);
    let c = fetch64(data, len - 8).wrapping_mul(mul);
    let d = fetch64(data, len - 16).wrapping_mul(K2);
    hash_len_16_with_mul(
        rotate(a.wrapping_add(b), 43)
            .wrapping_add(rotate(c, 30))
            .wrapping_add(d),
        a.wrapping_add(rotate(b.wrapping_add(K2), 18))
            .wrapping_add(c),
        mul,
    )
}

fn hash_len_33_to_64(data: &[u8]) -> u64 {
    let len = data.len();
    let mul = K2.wrapping_add((len as u64).wrapping_mul(2));
    let mut a = fetch64(data, 0).wrapping_mul(K2);
    let mut b = fetch64(data, 8);
    let c = fetch64(data, len - 24);
    let d = fetch64(data, len - 32);
    let e = fetch64(data, 16).wrapping_mul(K2);
    let f = fetch64(data, 24).wrapping_mul(9);
    let g = fetch64(data, len - 8);
    let h = fetch64(data, len - 16).wrapping_mul(mul);
    let u =
        rotate(a.wrapping_add(g), 43).wrapping_add(rotate(b, 30).wrapping_add(c).wrapping_mul(9));
    let v = (a.wrapping_add(g) ^ d).wrapping_add(f).wrapping_add(1);
    let w = bswap64((u.wrapping_add(v)).wrapping_mul(mul)).wrapping_add(h);
    let x = rotate(e.wrapping_add(f), 42).wrapping_add(c);
    let y = bswap64((v.wrapping_add(w)).wrapping_mul(mul))
        .wrapping_add(g)
        .wrapping_mul(mul);
    let z = e.wrapping_add(f).wrapping_add(c);
    a = bswap64((x.wrapping_add(z)).wrapping_mul(mul).wrapping_add(y)).wrapping_add(b);
    b = shift_mix(
        (z.wrapping_add(a))
            .wrapping_mul(mul)
            .wrapping_add(d)
            .wrapping_add(h),
    )
    .wrapping_mul(mul);
    b.wrapping_add(x)
}

fn weak_hash_len_32_with_seeds(data: &[u8], offset: usize, a: u64, b: u64) -> (u64, u64) {
    let w = fetch64(data, offset);
    let x = fetch64(data, offset + 8);
    let y = fetch64(data, offset + 16);
    let z = fetch64(data, offset + 24);
    let a = a.wrapping_add(w);
    let b = rotate(b.wrapping_add(a).wrapping_add(z), 21);
    let c = a;
    let a = a.wrapping_add(x).wrapping_add(y);
    let b = b.wrapping_add(rotate(a, 44));
    (a.wrapping_add(z), b.wrapping_add(c))
}

fn hash_len_16(u: u64, v: u64) -> u64 {
    hash_len_16_with_mul(u, v, 0x9ddfea08eb382d69)
}

fn hash_len_16_with_mul(u: u64, v: u64, mul: u64) -> u64 {
    let mut a = (u ^ v).wrapping_mul(mul);
    a ^= a >> 47;
    let mut b = (v ^ a).wrapping_mul(mul);
    b ^= b >> 47;
    b.wrapping_mul(mul)
}

fn fetch64(data: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(data[offset..offset + 8].try_into().expect("8 字节"))
}

fn fetch32(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().expect("4 字节"))
}

fn rotate(value: u64, shift: u32) -> u64 {
    if shift == 0 {
        value
    } else {
        (value >> shift) | (value << (64 - shift))
    }
}

fn shift_mix(value: u64) -> u64 {
    value ^ (value >> 47)
}

fn bswap64(value: u64) -> u64 {
    value.swap_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 空输入固定返回 K2。
    #[test]
    fn empty_city_hash_is_k2() {
        assert_eq!(city_hash64(&[]), K2);
    }

    /// 包名大小写不影响 ID，且 UTF-16 编码稳定。
    #[test]
    fn package_id_is_case_insensitive() {
        assert_eq!(
            package_id("Game/Content/SM_Test"),
            package_id("game/content/sm_test")
        );
        assert_ne!(
            package_id("Game/Content/SM_Test"),
            package_id("Game/Content/SM_Test2")
        );
        assert_ne!(package_id("Game/Content/SM_Test"), 0);
    }
}
