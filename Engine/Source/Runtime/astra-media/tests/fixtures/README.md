# Video geometry fixture

`dimensions-change.mp4` 是本仓生成的纯色测试片段，前 12 帧为红色
`32x32`，后 12 帧为蓝色 `64x32`，速率 `12 fps`，没有音频。
它用于验证播放期间拒绝尺寸变化，不包含外部素材。

可用 FFmpeg 重建：

```sh
ffmpeg -f lavfi -i color=c=red:s=32x32:r=12:d=1 -an -c:v libx264 -preset ultrafast -bf 0 -g 12 -f mpegts small.ts
ffmpeg -f lavfi -i color=c=blue:s=64x32:r=12:d=1 -an -c:v libx264 -preset ultrafast -bf 0 -g 12 -f mpegts wide.ts
cat small.ts wide.ts > change.ts
ffmpeg -i change.ts -c copy dimensions-change.mp4
rm small.ts wide.ts change.ts
```
