-- Offline legacy ACR/cnbg conversion; standard PAZ/SC loading remains native.
-- Not catalogued: fifteen name use-sites and parent native acceptance remain gated.
return {
    id="musica-yorino-cn", version=1, title="永不落幕的前奏诗旧补丁离线转换",
    apply=function(ctx)
        return ctx.load("common/yorino_entry.luau").apply(ctx, {
            names=44, dialogue=28358, width=1280, height=720,
            coverage={
                status="fifteen_name_use_sites_unlocated",
                row_index_base=0, message_speaker_rows=27,
                original_executable_raw_only_rows={27,43},
                unlocated_name_rows={28,29,30,31,32,33,34,35,36,37,38,39,40,41,42},
                dialogue_raw_source_occurrences=28358, native_script_files_validated=102,
                gate="Locate and verify the fifteen name use-sites through native display endpoints; raw EXE matches alone do not prove an endpoint. Parent must validate independent output with native readers and literal overlay/Tab/save/image composition before publication.",
            },
            -- These are payload CRCs pinned offline, not invented cnbg header fields.
            background_crc32={1242497977,2682413420,3754259598,3326280293,2852133049,
                1716791169,18048207,2269036656,2495441017,1620410953,342888811,2656658787,
                1785798599,3662395715,1285725960,1506401673,2881876346,3455895490,949022945,
                1119489060,931369560,2141044755,2926938256,818200612,249066011,3525415645,
                62905195,3177729526,2555900071,386053408,3767025320,925484774,3335806486,
                3747119052,2487054076,1566822862,877014806,589828690,2720632117,4259561475},
            inputs={
                {path="bg.paz",role="bg",size=1047777468,sha256="a2cf9f91de6d085862c2088c5c5edba90850a8cc00acf79790360b934f74366b"},
                {path="bg.pazA",size=1047861880,sha256="389d698cafeac65257cb9ea2f738e5519b75fd443e48684555b26d7819a928c1"},
                {path="bg.pazB",size=1048545376,sha256="ab6f824ad8076167a6c9070cd693d9e7a39ed4f236a11c476eb662e397755c81"},
                {path="bg.pazC",size=507676104,sha256="f9f88eca62a2cad8e3cce03af5343c8504a7e4f75d77509073770a99e50c04ab"},
                {path="bgm.paz",role="bgm",size=294114300,sha256="6b9659ec2ed6f669cf1de5a7affc45a4df11a362fe0eb3b152135301aab145e5"},
                {path="cnscr.paz",role="scr",size=2628004,sha256="2aa05f88a42bc31604283156e5952c0e3ece5a3a0bd5fff47dfb442bee4a42bf"},
                {path="st.paz",role="st",size=1048560804,sha256="5473ecb8fae1439bd417308b6ea00f4f9626a5e598a37acd176bce73c09abf61"},
                {path="st.pazA",size=570480784,sha256="2499e6163da896da0b0199f45af6ba6f9b997e0cbb9c4a769fbb251151c30da4"},
                {path="cnsys.paz",role="sys",size=78941532,sha256="731fbad68d5edc97e5181032f82d285494cce82446317af9263ca64426e3dda5"},
                {path="se.paz",role="se",size=113013876,sha256="e91eada317d32ef45118415ca12ebfcb392e968f5b8dc3fb494f6b07a6c21488"},
                {path="voice.paz",role="voice",size=548259252,sha256="c1df879381f2f9caf901c16e5d6dd6349f4d4cb1829d27d4159d5d7acf6c6996"},
                {path="mov.paz",role="mov",size=525755766,sha256="982ae247f034458f9a2813d44fab3e53eeb1fe3a8b9b0ee97dd9cf083d04a723"},
                {path="name.acr",patch=true,size=1578,sha256="bacb1f138f03addbc48314b3aa028644a96084e82f6eb0db633fc33999e81fb3"},
                {path="AsyncScript.acr",patch=true,size=2809326,sha256="229253bbbb57166e9fc671f9c29383db5d43ceb3e16b1ab2fba9a4cc2cbcb4d2"},
                {path="cnbg.paz",patch=true,size=147459780,sha256="94b2e1041325abb493f496b6a09e448520506bdcb6625c0c1f74845079adfef0"},
            },
        })
    end,
}
