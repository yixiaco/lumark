# 内置 Inno Setup 6.7.3 编译器

本目录是 Inno Setup 6.7.3 官方 Windows 发行版的最小命令行编译子集，供
`installer/pack-installer.ps1` 直接调用。构建机不需要安装 Inno Setup，也不需要
管理员权限。

- 来源：<https://jrsoftware.org/isdl.php>
- 版本：6.7.3（官方安装包 `innosetup-6.7.3.exe`）
- 许可：见同目录 `license.txt`（Inno Setup License）
- 二进制未修改；`.issig` 是官方编译器用于自校验的签名清单，不能删除
- `SHA256SUMS.txt` 记录本目录内文件哈希

`pack-installer.ps1` 默认执行：

```powershell
./installer/tools/innosetup/ISCC.exe /DAppVersion=0.1.0 ./installer/lumark.iss
```

不要用系统安装目录或 PATH 中的 `ISCC.exe` 替换本目录；否则构建会重新依赖
Inno Setup 安装。

## 商业许可

Inno Setup 6.7.3 未授权编译器会输出 `Non-commercial use only`。商业发布请按
<https://jrsoftware.org/isorder.php> 购买 Inno Setup 商业许可。该提示不影响
非商业使用，也不会把安装器标记为未授权。
