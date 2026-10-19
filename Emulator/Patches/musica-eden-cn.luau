-- Legacy DLL-only speaker names become ordinary native localization data.
-- Standard PAZ/GBK/SC/font behavior remains the native Musica load path.
return {
    id = "musica-eden-cn",
    version = 1,
    title = "eden 中文姓名资源",
    apply = function(ctx)
        return ctx.load("common/musica.luau").apply_names(ctx, {
            executable = "eden_en.exe",
            exe = { size = 2267992, sha256 = "7b0f4be87c1296e62b7425d2ee3470e337e51921c4e28adba67d4a3cfed50fc3" },
            dll = "eden_cn.dll",
            patch = { size = 35840, sha256 = "7e1822eefa8673ff29fbd7bc85e129a25b28a6b26e77a086fa0fc02506a9b8b8" },
            layout = {
                table_rva = 0x197720,
                targets = { 0x80b0, 0x80e0, 0x80f4, 0x80d8, 0x80c4, 0x80ec, 0x80cc, 0x80d0, 0x80b8 },
            },
            archives = {
                bg = { size = 1960446996, sha256 = "1d6bf147f39d01c710a50e9107d6b2fd0269a029f17c523ae4900d2ef6dbcaeb" },
                bgm = { size = 318409764, sha256 = "b7eb9e36c009a58b7b0f13b61e1b43a6645acff9aa63592d9f75748d61ca53fe" },
                scr = { size = 749972, sha256 = "713fd44396e32069bebf372f28ec43a9522d3db01262b7684d0d96b06757bdc7" },
                st = { size = 100, sha256 = "68dfefbd91fb89117cecff28d46f6b5320c0b2bb02d84a2147ed8591eb403f85" },
                sys = { size = 49646452, sha256 = "25779f07a7ed254b35e65fbf159333b3578f185a27eb7433f5bc1684efd062dd" },
                se = { size = 5206220, sha256 = "c9d4f7b5814608fb39c3059c899318bc1878c2fe90c0060322dc85f24131a77e" },
                voice = { size = 179445068, sha256 = "9089d85aeab5edc69fd4426187fd2a44d9539f53831e5aee5bfbe630124125f3" },
                mov = { size = 171098372, sha256 = "b9d71ceb0b53a1db9dfb5f2f87bf3ea4f975edac10d43fd5e707abb1912e7a0d" },
            },
        })
    end,
}
