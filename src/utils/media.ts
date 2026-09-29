// The one list of file types the UI lets in. The Rust side has no list: anything ffprobe reads works,
// so this only gates dialogs and Finder drops. Keep README / CLAUDE.md in step.
export const VIDEO_EXT = ["mp4", "mov", "m4v", "mkv", "webm", "avi", "mts", "m2ts", "3gp", "ts", "mpg", "mpeg", "wmv", "flv", "mxf"];
export const AUDIO_EXT = ["mp3", "m4a", "aac", "wav", "aiff", "aif", "flac", "ogg", "oga", "opus", "caf", "m4b", "wma"];
export const IMAGE_EXT = ["png", "jpg", "jpeg", "webp", "bmp", "tif", "tiff"];
export const MEDIA_EXT = [...VIDEO_EXT, ...AUDIO_EXT, ...IMAGE_EXT];

/** Lower-cased extension without the dot; "" when there is none. */
export const extOf = (path: string): string => {
  const name = path.split("/").pop() ?? "";
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(dot + 1).toLowerCase() : "";
};
export const isMediaPath = (path: string): boolean => MEDIA_EXT.includes(extOf(path));
