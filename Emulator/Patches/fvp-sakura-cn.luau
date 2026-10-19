-- This entry converts the legacy filter.dll packaging, not RFVP's standard formats.
-- The resulting offset table, native textures, fonts and ASS use ordinary RFVP consumers.
local resources = {
    {"graph/a_bu_cp1",1673,803865},{"graph/a_bus_s1",805538,25674},
    {"graph/a_cg_p3",831212,5193},{"graph/a_cg_s1",836405,1122912},
    {"graph/a_mem_s1",1959317,972958},{"graph/a_mov_s1",2932275,954533},
    {"graph/a_snd_s1",3886808,1101660},{"graph/a_top_s1",4988468,1008395},
    {"graph/bl_char",5996863,105780},{"graph/bl_mask",6102643,21722},
    {"graph/fc_back_help",6124365,1870},{"graph/fc_load_help",6126235,2896},
    {"graph/fc_lock_help",6129131,2500},{"graph/fc_play_help",6131631,4337},
    {"graph/fc_qload_help",6135968,4321},{"graph/fc_qsave_help",6140289,2769},
    {"graph/fc_save_help",6143058,2813},{"graph/fc_skip_help",6145871,1830},
    {"graph/fc_system_help",6147701,4041},{"graph/fc_voice_help",6151742,2441},
    {"graph/fc_wincls_help",6154183,3388},{"graph/menu_bg",6157571,18435},
    {"graph/menu_del_mes",6176006,4122},{"graph/menu_exit_mes",6180128,4491},
    {"graph/menu_init_mes",6184619,3694},{"graph/menu_no_1",6188313,632},
    {"graph/menu_no_2",6188945,2232},{"graph/menu_ow_mes",6191177,3962},
    {"graph/menu_qload_mes",6195139,4458},{"graph/menu_title_mes",6199597,4208},
    {"graph/menu_undo_mes",6203805,5296},{"graph/menu_yes_1",6209101,574},
    {"graph/menu_yes_2",6209675,1917},{"graph/opt_m_pts1",6211592,926158},
    {"graph/opt_m_pts2",7137750,408508},{"graph/opt_o_pts1",7546258,915956},
    {"graph/opt_o_pts2",8462214,403720},{"graph/opt_parts",8865934,117858},
    {"graph/opt_s_pts1",8983792,59452},{"graph/opt_s_pts2",9043244,41264},
    {"graph/opt_sheet1",9084508,1047776},{"graph/opt_sheet2",10132284,1043719},
    {"graph/opt_sheet3",11176003,1016048},{"graph/slf_load_bg",12192051,1186985},
    {"graph/slf_save_bg",13379036,1211218},{"graph/ssel_bg",14590254,675509},
    {"graph/startmoji_t01",15265763,8876},{"graph/startmoji_t02",15274639,8279},
    {"graph/startmoji_t10",15282918,8881},{"graph/startmoji_t11",15291799,9763},
    {"graph/sys_config_M",15301562,7730},{"graph/sys_exit_M",15309292,8932},
    {"graph/sys_hidewin_M",15318224,8991},{"graph/sys_load_M",15327215,8555},
    {"graph/sys_return_M",15335770,8756},{"graph/sys_save_M",15344526,8896},
    {"graph/sys_title_M",15353422,8140},{"graph/sys_undo_M",15361562,8136},
    {"graph/sys_window_front",15369698,82202},{"graph/title_logo1",15451900,195901},
    {"graph/warning",15647801,793196},{"graph_vis/ETC_e000b",16440997,1479117},
    {"graph_vis/ETC_e001a",17920114,1858028},{"graph_vis/ETC_e012b",19778142,2393913},
    {"graph_vis/ETC_e012c",22172055,2498171},{"graph_vis/ETC_e012d",24670226,2267182},
    {"graph_vis/ETC_e201a",26937408,713533},{"graph_vis/ETC_e201b",27650941,799283},
    {"graph_vis/ETC_e201c",28450224,697226},{"graph_vis/ETC_e201d",29147450,698786},
    {"graph_vis/ETC_e302b",29846236,1788594},
}
local fonts = {
    {"AaGuiqi.ttf","2e87ef74cd6ead1eb4c1488e015c29e9fac4249b99088fb9cf8ae825da2ef937"},
    {"HanaMinA.ttf","7ca87559414474848fc513a8d8eb42d5b0f8194c30c2abdfbfedff5398ec88d7"},
    {"RyuminPro-Light.otf","a76fe041e01658beb32b9454f9e45ee780dd096611f0fe413b4c2e5ec06801e7"},
    {"Yu-MinCho-Light.ttf","e107022b5873c3b8e4cbdbaccdd0432f83517529862af132911e62921abe5bd7"},
}
local tracks = {
    {clock="movie",resource="movie/01.wmv",ass="subtitle/vd1.ass",sha256="ce1bbd96d654e160d798978c1caa0482dfa24f6eba426ed845742387da347a3b"},
    {clock="movie",resource="movie/02.wmv",ass="subtitle/vd2.ass",sha256="1e1c5f53715d6749620bfc8bfd50398ad1f7e6ee4becfa5943976c198cf37a38"},
    {clock="bgm",resource="BGM/073",ass="subtitle/ed1.ass",sha256="fe02f13eed7fb5ea3d7b937dc17cec171ac14d63494d2d714e72255ba656d7aa"},
    {clock="bgm",resource="BGM/074",ass="subtitle/ed2.ass",sha256="b316c9feabadcb76bb4651b24b248b0a59fe6a13032a2094de5a6c2907c4e523"},
}
local function pinned(ctx, root, path, hash)
    local source = ctx.open(root, path)
    if source:sha256() ~= hash then ctx.fail("UNSUPPORTED_INPUT", "FVP input fingerprint mismatch: " .. path) end
    return source
end
local function parent(ctx, path)
    local dir = string.match(path, "^(.*)/[^/]+$")
    if dir then ctx.mkdir(dir) end
end
return {
    id="fvp-sakura-cn", version=1, title="樱花萌放旧补丁离线转换",
    apply=function(ctx)
        local fvp = ctx.load("common/fvp.luau")
        local copy = ctx.load("common/copy.luau")
        local hcb = pinned(ctx,"source","Sakura.hcb","946877dd0ed8fbf318ba5c73d20afd46b9dcdc200aa7e8edc610c437cc1c789b")
        local dat = pinned(ctx,"patch","patch.dat","c642c8e06e29c991aaed22378a3e3e0981ca61ce83dbfea087c267cf3c5cd391")
        pinned(ctx,"patch","filter.dll","8ab7fb019b749a272d13de42375f819bb0cbd0d721ea1355f911147a87142441")
        local pack = pinned(ctx,"patch","patch.bin","04518ff4563ea54dc2f50b3c78c8b3d058b2b2c2d42ac4d35911cb4ed2d6096c")
        for _, font in fonts do pinned(ctx,"patch","subtitle/fonts/" .. font[1],font[2]) end
        local ass_sources = {}
        for _, track in tracks do ass_sources[track.ass] = pinned(ctx,"patch",track.ass,track.sha256) end
        -- Exact physical slices from this DLL mapping and this BIN fingerprint. No generic BIN parser.
        copy.files(ctx,"source",{
            "bgm.bin","graph.bin","graph_bg.bin","graph_bs.bin","graph_sd.bin",
            "graph_vis.bin","graph_vish.bin","se.bin","se_env.bin","se_sys.bin","voice.bin",
            "movie/01.wmv","movie/02.wmv","cursor1.ani","cursor2.ani",
        })
        local records = fvp.dictionary(ctx, dat)
        if #records ~= 57053 then ctx.fail("INTEGRITY", "FVP active dictionary count mismatch") end
        local patched = fvp.warning(ctx,hcb,{site=0x74daa,original_call=0x37e50,helper=0x373a5,wait_syscall=137,yield_syscall=133})
        local script = ctx.create("Sakura.hcb")
        script:write(0,patched)
        script:close()
        fvp.write_table(ctx,"astra-translations.bin",hcb:sha256(),ctx.hash.sha256(patched),records)
        for index, resource in resources do
            ctx.check_cancelled()
            parent(ctx,resource[1])
            local output = ctx.create(resource[1])
            output:write(0,pack:read(resource[2],resource[3]))
            output:close()
            ctx.progress("FVP localized native resources",index,#resources)
        end
        ctx.mkdir("font")
        ctx.mkdir("subtitle/fonts")
        local font_paths = {}
        for _, font in fonts do
            local path = "subtitle/fonts/" .. font[1]
            ctx.copy("patch",path,path)
            ctx.copy("patch",path,"font/" .. font[1])
            table.insert(font_paths,path)
        end
        local output_tracks = {}
        for _, track in tracks do
            local ass = ass_sources[track.ass]
            local text = ctx.encoding.decode(ass:read(0,ass:size()),"utf8")
            if string.sub(text,1,3) == "\239\187\191" then text = string.sub(text,4) end
            local output = ctx.create(track.ass)
            output:write(0,ctx.encoding.encode(text,"utf8"))
            output:close()
            table.insert(output_tracks,{clock=track.clock,resource=track.resource,ass=track.ass,fonts=font_paths})
        end
        local map = ctx.create("astra-subtitles.json")
        map:write(0,buffer.fromstring(ctx.json.encode({version=1,tracks=output_tracks})))
        map:close()
        return {plugin_id="astra.emu.fvp",configuration={script_encoding="shift_jis",
            translation_table="astra-translations.bin",subtitle_map="astra-subtitles.json"}}
    end,
}
