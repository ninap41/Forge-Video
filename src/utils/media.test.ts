import { describe, expect, it } from "vitest";
import { AUDIO_EXT, IMAGE_EXT, MEDIA_EXT, VIDEO_EXT, extOf, isMediaPath } from "./media";

describe("media extensions", () => {
  it("lists each extension once and covers the common audio, video and image types", () => {
    expect(new Set(MEDIA_EXT).size).toBe(MEDIA_EXT.length);
    expect(MEDIA_EXT).toHaveLength(VIDEO_EXT.length + AUDIO_EXT.length + IMAGE_EXT.length);
    for (const e of ["mp4", "mov", "mkv", "webm", "avi", "mts", "3gp", "wmv"]) expect(VIDEO_EXT).toContain(e);
    for (const e of ["mp3", "m4a", "aac", "wav", "aiff", "aif", "flac", "ogg", "opus", "caf"]) expect(AUDIO_EXT).toContain(e);
    for (const e of ["png", "jpg", "jpeg", "webp", "bmp", "tiff"]) expect(IMAGE_EXT).toContain(e);
  });

  it("extOf is case-insensitive and ignores dot-files and dotted folders", () => {
    expect(extOf("/Music/Song.MP3")).toBe("mp3");
    expect(extOf("/v.1/clip")).toBe("");
    expect(extOf("/tmp/.hidden")).toBe("");
    expect(extOf("archive.tar.gz")).toBe("gz");
  });

  it("isMediaPath accepts listed types only", () => {
    expect(isMediaPath("/a/b.OGG")).toBe(true);
    expect(isMediaPath("/a/notes.txt")).toBe(false);
    expect(isMediaPath("/a/b.forgevideo")).toBe(false);
  });
});
