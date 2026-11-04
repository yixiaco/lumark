//! FModel/CUE4Parse 材质参数触发名与名称正则。
//!
//! 触发名表来自 CUE4Parse `CMaterialParams2`（Apache-2.0，
//! <https://github.com/FabianFG/CUE4Parse>）的首 UV 层条目；Lumark 只使用
//! 首层（单 UV 近似），并按 FModel `Material.Setup` 的优先级选择贴图。
//! 这里只保存数据与纯匹配函数，材质链求值仍放在 `material_graph`。

/// FModel `CMaterialParams2.Diffuse[0]`：基础色参数名（顺序即优先级）。
pub(crate) const DIFFUSE_TRIGGERS: &[&str] = &[
    "Trunk_BaseColor",
    "ShadedDiffuse",
    "LitDiffuse",
    "Base Colour",
    "CLRM",
    "Diffuse_Map",
    "Base Color",
    "Base_Tex",
    "BaseTex",
    "ColorTexture",
    "Background Diffuse",
    "BG Diffuse Texture",
    "Diffuse",
    "Diffuse_1",
    "DiffuseTexture",
    "DiffuseMap",
    "Diffuse A",
    "Base Color Map",
    "DesatTexture",
    "Diffuse A Map",
    "Diffuse Top",
    "Diffuse Side",
    "Base Diffuse",
    "Diffuse Base",
    "Diffuse Base Map",
    "Diffuse Color Map",
    "BaseColor_Tex",
    "UDR",
    "DiffuseLayer1",
    "1 - Albedo",
    "albedo",
    "Albedo",
    "Aldebo",
    "ALB",
    "TextureAlbedo",
    "AlbedoTex",
    "Color_Texture",
    "color",
    "Base_D",
    "Tex_BaseColor",
    "AlbedoColour",
    "BCR",
    "Base Texture",
    "BaseColour",
    "Color Map",
    "Base Color - Texture",
    "BaseColor Map",
    "VT_Tex_SkinBase_BC",
    "Base Color Texture",
    "BaseColorTexture",
    "BaseColor_Texture",
    "Base_Color",
    "BaseColor",
    "Basecolor",
    "Tex_BC",
    "TexA_BC",
    "BaseDiffuse",
    "Base Texture Color",
    "BaseColorA",
    "BC",
    "BCA",
    "BC_Map",
    "Base Map",
    "BCE",
    "Color",
    "CO",
    "CO_",
    "CO_1",
    "Base_CO",
    "Base Color + Linework",
    "Bc",
    "BaseColorVT",
    "Tex_Color",
    "Color Tex",
    "TexColor",
    "AlbedMap",
    "Tex_Colormap",
    "ColorMap",
    "Main_T_BaseColor",
    "Base_Texture",
    "T_BaseColor",
    "Decal_Texture",
    "PetalDetailMap",
    "CliffTexture",
    "M1_T_BC",
    "Skin Diffuse",
    "Color_MAIN",
    "MainTex",
    "ToneRimTex",
    "C",
    "Primary Base Color",
    "BaseColor_NonVT",
    "L0_B/BM",
    "B/BM",
    "NRO_Base_Color",
    "BaeColor_Tex",
    "Layer00_BaseColor_Tex",
    "01. Diffuse Texture",
    "BaseMap",
    "Tex_SkinBase_BC",
    "Tex_Core_Layer_Albedo",
    "BC_Texture",
    "RGBMap",
    "RGB_Map",
    "MW_TextureBaseColor",
    "Texture",
    "ColorOpacity",
    "Simple_BaseColor_Texture",
    "AlbedoTexture",
    "Albedo Texture",
    "BC_LUT",
    "BCO",
    "CR",
    "C/R",
    "BaseColor_Tex(a:mask)",
    "Base Color Map VT",
    "Basecolor_VT",
    "BaseColor_Map",
    "BaceColor_Map",
    "ColorAo",
    "RGBMask",
    "Color & Alpha",
    "BaseColor - Texture",
    "Clothing Diffuse",
    "Base Color [Standard]",
    "Base Color VT",
    "BaseColorRoughness",
    "BaseColorAlpha",
    "Color Map (Packed)",
    "D(ID)",
    "BC Texture (BC/BCO)",
    "BCM BaseColor|Metallic Map",
    "Color_Main",
    "Texture_BC",
    "Virtual Texture Base Color",
    "DF",
    "L01.BaseColor",
    "BCM",
    "WGAD_Map",
    "OSGB",
    "Decal Base Color & Alpha",
    "Tex_Core_Layer_AO",
];

/// FModel `CMaterialParams2.Normals[0]`：法线参数名。
pub(crate) const NORMAL_TRIGGERS: &[&str] = &[
    "Trunk_Normal",
    "T_Normal",
    "normal",
    "normalmap",
    "NRRO",
    "Base Normal",
    "BaseNormalMap",
    "Normal Map",
    "Base Normal Map",
    "Normals",
    "Normal",
    "NormalA",
    "NormalTexture",
    "Normal Texture",
    "Normal_Texture",
    "NormalMap",
    "Normal Tex",
    "Normal A Map",
    "Normals Top",
    "Normals Side",
    "Fallback Normal",
    "Base_N",
    "BaseNormal_Tex",
    "BaseNormal_NonVT",
    "Base_Normal",
    "BaseNormal",
    "Normal Base",
    "NormalVT",
    "TextureNormal",
    "Tex_BakedNormal",
    "TexNor",
    "Normal VT",
    "BakedNormalMap",
    "3 - Baked Normal",
    "Base Texture Normal",
    "Normal Base Map",
    "Tex_NM",
    "TexA_NM",
    "Normal_Tex",
    "N",
    "NM",
    "NM_1",
    "Base_NM",
    "NRM",
    "T_NRM",
    "M1_T_NRM",
    "Base NRM",
    "NRM Base",
    "NRH",
    "MW_TextureNormal",
    "VT_Tex_SkinBase_N",
    "Texture A Normal",
    "CliffNormal",
    "Skin Normal",
    "Normal_MAIN",
    "Main_T_Normal(B：SSSMask)",
    "PBREmissiveTex",
    "Primary Normal Map",
    "ORN",
    "NRO_Base_Normal",
    "Layer00_Normal_Tex",
    "Tex_SkinBase_N",
    "Tex_Core_Layer_Normal",
    "Simple_Normal_Texture",
    "BaseNormal Texture",
    "NOH",
    "N/O/H",
    "NMH",
    "NWO",
    "Normal_VT",
    "NRM_Map",
    "Normal - Texture",
    "Clothing Normal",
    "Normal [Standard]",
    "NCD_Mask",
    "Normal Map (Packed)",
    "N2R(M)",
    "Normal Texture (N)",
    "Normal(HrNM)_Texture",
    "VT Normal",
    "L01.Normal.Metallic.Roughness",
    "NRA",
];

/// FModel `CMaterialParams2.SpecularMasks[0]`：高光遮罩参数名。
///
/// 注意这里包含 `AO`：BNB 等资产把 AO/ORM 贴图也用作 specular mask
/// （G=金属度/腔体、B=粗糙度），与 FModel 的行为一致。
pub(crate) const SPECULAR_TRIGGERS: &[&str] = &[
    "Trunk_Specular",
    "PackedTexture",
    "SpecularMasks",
    "Specular",
    "SpecMap",
    "T_Specular",
    "Specular Top",
    "Specular Side",
    "MG",
    "ORM",
    "MRAE",
    "MRAS",
    "MRAO",
    "MRA",
    "MRAO_Map",
    "MRA A",
    "MRS",
    "LP",
    "LP_1",
    "Base_LP",
    "Base_R*",
    "MRO_Tex",
    "TextureRMA",
    "Tex_MultiMask",
    "Tex_Multi",
    "TexMRC",
    "TexMRA",
    "TexRCN",
    "MultiMaskMap",
    "MRO Map",
    "MROA Map",
    "MRO",
    "Base_ORM",
    "Base_SRO",
    "Base Texture RMAO",
    "Skin SRXO",
    "SRXO_Mask",
    "SRXO",
    "SROA",
    "SR",
    "SRO Map",
    "SRM",
    "SC_Map",
    "AO",
    "Pack",
    "PAK",
    "T_PAK",
    "M1_T_PAK",
    "2 - Packed mask (MRAO)",
    "RoughnessMaterial_Mask",
    "Packed Tex",
    "Packed Texture",
    "VT_Tex_SkinBase_ORM",
    "Cliff Spec Texture",
    "PhysicalMap",
    "KizokMap",
    "Roughness_MAIN",
    "Main_T_MGA",
    "Tex_CH",
    "TexA_CH",
    "ARMMap",
    "REM",
    "Primary ARME",
    "CombineTex(HRA)",
    "ORN",
    "TPA_SpecColorTex",
    "Tex_RME",
    "CompVT",
    "MetalRoughOcc_Tex",
    "HRM\\ORM Map",
    "Layer00_MetalRoughOccDp_Tex",
    "Tex_SkinBase_ORM",
    "Tex_Core_Layer_AO",
    "RGB[AO/R/Metallic]_Texture",
    "MRO - Texture",
    "Simple_OcclRoughMet_Texture",
    "P (NoneVT)",
    "Pack Tex1",
    "CASR",
    "Roughness_VT",
    "OcclusionRoughnessMetallicTexture",
    "Clothing ORM",
    "ORC [Standard]",
    "ORME_Tex",
    "Roughness VT",
    "RMAO VT",
    "Mix(AO,Rough,Mask,Metal)",
    "Texture_S",
    "MetallicRoughnessOcclusionSpecularTexture",
    "Virtual Texture ORM",
];

/// FModel `CMaterialParams2.Emissive[0]`：自发光参数名。
pub(crate) const EMISSIVE_TRIGGERS: &[&str] = &[
    "Emissive",
    "EmissiveTexture",
    "EmissiveColorTexture",
    "EmissiveColor",
    "EmissiveMask",
    "EmmisiveColor_A",
    "TextureEmissive",
    "TexEm",
    "Main_T_Emissive",
    "VT Emissive",
];

/// FModel `CMaterialParams2.DiffuseColors[0]`：基础色 tint 参数名。
pub(crate) const DIFFUSE_COLOR_TRIGGERS: &[&str] = &[
    "ColorMult",
    "Color_mul",
    "Base Color",
    "BaseColor",
    "Color",
    "color",
    "DiffuseColor",
    "tex1_CO",
    "ColorA",
    "ALB",
    "AlbedoColor",
    "Layer A Tint",
];

/// FModel `CMaterialParams2.EmissiveColors[0]`：自发光 tint 参数名。
pub(crate) const EMISSIVE_COLOR_TRIGGERS: &[&str] = &[
    "Emissive",
    "Emissive Color",
    "EmissiveColor",
    "EMI",
    "EmColor",
    "Color",
];

/// FModel `Material.Setup` 的 AO 贴图参数名。
pub(crate) const AO_TRIGGERS: &[&str] = &["M", "AEM", "AO"];

/// 粗糙度下限参数名。
pub(crate) const ROUGHNESS_MIN_TRIGGERS: &[&str] = &["RoughnessMin", "SpecRoughnessMin"];

/// 粗糙度上限参数名。
pub(crate) const ROUGHNESS_MAX_TRIGGERS: &[&str] = &["RoughnessMax", "SpecRoughnessMax"];

/// 单值粗糙度参数名：FModel 以 `value ± value/2` 生成区间。
pub(crate) const ROUGHNESS_TRIGGERS: &[&str] = &[
    "Rough",
    "Roughness",
    "Ro Multiplier",
    "RO_mul",
    "Roughness_Mult",
];

/// 自发光倍率标量参数名。
pub(crate) const EMISSIVE_MULT_TRIGGERS: &[&str] = &[
    "emissive mult",
    "Emissive_Mult",
    "EmissiveIntensity",
    "EmissionIntensity",
];

/// 自发光倍率颜色参数名（取 R 分量）。
pub(crate) const EMISSIVE_MULT_COLOR_TRIGGERS: &[&str] =
    &["Emissive Multiplier", "EmissiveMultiplier"];

/// AO 颜色增强参数名（RGB 颜色 + A 指数）。
pub(crate) const AO_BOOST_TRIGGERS: &[&str] = &["Skin Boost Color And Exponent"];

/// FModel `RegexDiffuse`：贴图名/参数名是否像基础色。
pub(crate) fn matches_diffuse(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("diff")
        || lower.contains("_tex")
        || lower.contains("albedo")
        || lower.contains("basecolor")
        || lower.contains("base_color")
        || lower.ends_with("_d")
        || lower.ends_with("_dif")
        || lower.ends_with("_dm")
        || lower.ends_with("_c")
        || lower.ends_with("_cm")
        || lower.ends_with("_ds")
        || lower.ends_with("_da")
}

/// FModel `RegexNormals`：贴图名/参数名是否像法线。
pub(crate) fn matches_normals(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("no_")
        || lower.contains("norm")
        || lower.ends_with("_n")
        || lower.ends_with("_nm")
        || lower.ends_with("_nrm")
}

/// FModel `RegexSpecularMasks`：贴图名/参数名是否像高光遮罩。
pub(crate) fn matches_specular_masks(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("sp_")
        || lower.contains("specu")
        || lower.contains("_s_")
        || lower.contains("mr")
        || lower.contains("rm")
        || lower.ends_with("_s")
        || lower.ends_with("_lp")
        || lower.ends_with("_pak")
}

/// FModel `RegexEmissive`：贴图名/参数名是否像自发光。
pub(crate) fn matches_emissive(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("emiss") || lower.ends_with("_e") || lower.ends_with("_em")
}
