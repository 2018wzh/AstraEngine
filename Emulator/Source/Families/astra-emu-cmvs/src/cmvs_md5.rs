use astra_emu_sdk::CoreError;

#[allow(dead_code)]
pub(crate) fn compute(input: [u32; 4], variant: &str) -> Result<[u32; 4], CoreError> {
    let (mut a, mut b, mut c, mut d): (u32, u32, u32, u32) = match variant {
        "A" | "Chrono" => (0xc74a_2b01, 0xe7c8_ab8f, 0xd8be_dc4e, 0x7302_a4c5),
        "B" => (0x53fe_9b2c, 0xf2c9_3ea8, 0xee81_ba59, 0xa2c8_973e),
        "Memoria" => (0xa794_63f9, 0xb6e7_55c5, 0xc696_af21, 0x6983_e978),
        "Natsu" => (0x63fe_9a7c, 0xc2b9_3e98, 0xef91_ba5c, 0x72c9_a82e),
        "Aoi" => (0xc74a_2b02, 0xe7c8_ab8f, 0x38be_bc4e, 0x7531_a4c3),
        "Mirai" => (0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476),
        _ => {
            return Err(CoreError::invalid(
                "ASTRA_EMU_CMVS_MD5_VARIANT",
                "CMVS MD5 variant is unsupported",
            ))
        }
    };
    let initial = [a, b, c, d];
    let mut words = [0u32; 16];
    words[..4].copy_from_slice(&input);
    words[4] = 0x80;
    words[14] = 0x80;
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    const K: [u32; 64] = [
        0xd76a_a478,
        0xe8c7_b756,
        0x2420_70db,
        0xc1bd_ceee,
        0xf57c_0faf,
        0x4787_c62a,
        0xa830_4613,
        0xfd46_9501,
        0x6980_98d8,
        0x8b44_f7af,
        0xffff_5bb1,
        0x895c_d7be,
        0x6b90_1122,
        0xfd98_7193,
        0xa679_438e,
        0x49b4_0821,
        0xf61e_2562,
        0xc040_b340,
        0x265e_5a51,
        0xe9b6_c7aa,
        0xd62f_105d,
        0x0244_1453,
        0xd8a1_e681,
        0xe7d3_fbc8,
        0x21e1_cde6,
        0xc337_07d6,
        0xf4d5_0d87,
        0x455a_14ed,
        0xa9e3_e905,
        0xfcef_a3f8,
        0x676f_02d9,
        0x8d2a_4c8a,
        0xfffa_3942,
        0x8771_f681,
        0x6d9d_6122,
        0xfde5_380c,
        0xa4be_ea44,
        0x4bde_cfa9,
        0xf6bb_4b60,
        0xbebf_bc70,
        0x289b_7ec6,
        0xeaa1_27fa,
        0xd4ef_3085,
        0x0488_1d05,
        0xd9d4_d039,
        0xe6db_99e5,
        0x1fa2_7cf8,
        0xc4ac_5665,
        0xf429_2244,
        0x432a_ff97,
        0xab94_23a7,
        0xfc93_a039,
        0x655b_59c3,
        0x8f0c_cc92,
        0xffef_f47d,
        0x8584_5dd1,
        0x6fa8_7e4f,
        0xfe2c_e6e0,
        0xa301_4314,
        0x4e08_11a1,
        0xf753_7e82,
        0xbd3a_f235,
        0x2ad7_d2bb,
        0xeb86_d391,
    ];
    for i in 0..64 {
        let (f, g) = if i < 16 {
            ((b & c) | (!b & d), i)
        } else if i < 32 {
            ((d & b) | (!d & c), (5 * i + 1) % 16)
        } else if i < 48 {
            (b ^ c ^ d, (3 * i + 5) % 16)
        } else {
            (c ^ (b | !d), (7 * i) % 16)
        };
        let next = a
            .wrapping_add(f)
            .wrapping_add(K[i])
            .wrapping_add(words[g])
            .rotate_left(S[i])
            .wrapping_add(b);
        a = d;
        d = c;
        c = b;
        b = next;
    }
    a = a.wrapping_add(initial[0]);
    b = b.wrapping_add(initial[1]);
    c = c.wrapping_add(initial[2]);
    d = d.wrapping_add(initial[3]);
    Ok(match variant {
        "A" => [d, b, c, a],
        "Chrono" => [
            c ^ 0x45a7_6c2f,
            b.wrapping_sub(0x5ba1_7fcb),
            a ^ 0x79ab_e8ad,
            d.wrapping_sub(0x1c08_561b),
        ],
        "B" => [
            b ^ 0x4987_5325,
            c.wrapping_add(0x54f4_6d7d),
            d ^ 0xad79_48b7,
            a.wrapping_add(0x1d06_38ad),
        ],
        "Memoria" => [b, c, d, a],
        "Natsu" => [
            b.wrapping_add(0x4587_6329),
            c ^ 0x54f3_6d6c,
            d.wrapping_add(0x4387_a749),
            a ^ 0xe3f9_a742,
        ],
        "Aoi" => [
            c ^ 0x53a7_6d2e,
            b.wrapping_add(0x5bb1_7fda),
            a.wrapping_add(0x6853_e14d),
            d ^ 0xf5c6_a9a3,
        ],
        _ => [a, b, c, d],
    })
}

#[cfg(test)]
mod tests {
    use super::compute;

    #[test]
    fn mirai_matches_standard_md5_for_sixteen_zero_bytes() {
        assert_eq!(
            compute([0; 4], "Mirai").unwrap(),
            [0x3613_e74a, 0xbff9_4be4, 0x2e75_d279, 0xa518_4823]
        );
    }

    #[test]
    fn unknown_variant_is_blocking() {
        assert_eq!(
            compute([0; 4], "unknown").unwrap_err().code(),
            "ASTRA_EMU_CMVS_MD5_VARIANT"
        );
    }
}
