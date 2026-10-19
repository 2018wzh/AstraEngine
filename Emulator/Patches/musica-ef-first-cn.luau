-- The installed edition has no r18_cn directory. Optional rank-5 archives
-- require their own native overlay pairing rather than copying an unknown tree.
-- DLL RT_BITMAP1/2 belong to Win32 About; dialog139 names original save slots.
return {
    id = "musica-ef-first-cn",
    version = 1,
    title = "ef first 中文姓名资源",
    apply = function(ctx)
        return ctx.load("common/musica.luau").apply_names(ctx, {
            executable = "ef_first_AA.exe",
            exe = { size = 2048104, sha256 = "9824732313d13136cb2300f33038a655ba480ae01cefc2749028d1b1748b4f9e" },
            dll = "ef_first_cn_AA.dll",
            patch = { size = 260200, sha256 = "1822406b6317483e1cc50094ac0f6104dbccb560397e934f6f5e2130d9fbe7cf" },
            reject_r18 = true,
            layout = {
                table_rva = 0x1a9f88,
                targets = { 0x2154, 0x2158, 0x2160, 0x2164, 0x216c, 0x2174, 0x2178, 0x2180, 0x2188, 0x218c },
            },
            archives = {
                bg = { size = 1272342060, sha256 = "c2b0b02561aea66a6e32e0df1c7a5b2d659a68899346f59693afc0cd17b7847a" },
                bgm = { size = 51733692, sha256 = "1f525b7eb8ca89f7ad0c04535d5639aec5099db7afef5d334f49bba8a8c50b94" },
                scr = { size = 405268, sha256 = "462210b28fc1fd4c7e5aabfcefde3e67d9bb4bcbdb714ae2181eef5e7f9e99d7" },
                st = { size = 121894852, sha256 = "a5995d8989c0a19de46389b50160dfc2477fef188191f0eb3f6a69a020732b49" },
                sys = { size = 20932372, sha256 = "5a6ddb063181ddcb84210d695f463be8b0e68e60a7c81812d8bc029fc3621d01" },
                se = { size = 6494884, sha256 = "c9fa3b12a38ad671585b3cb0b211af5be68ae04a80ec9634cc18350ea1391daf" },
                voice = { size = 217589044, sha256 = "825bc4ee720bfd8eb7c9116e65dad15722fdcfc43d814e003027f38130d5dacd" },
                mov = { size = 51003452, sha256 = "19963c3a41ea6a457932d3c59db29d635b2abcba0dbb9e4bb19657e26358b89d" },
            },
        })
    end,
}
