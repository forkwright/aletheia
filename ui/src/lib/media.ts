// File type utilities shared between InputBar and AttachmentBar
export const IMAGE_TYPES = ["image/jpeg", "image/png", "image/gif", "image/webp"];
export const DOC_TYPES = ["application/pdf"];
export const TEXT_TYPES = [
  "text/plain", "text/csv", "text/markdown", "text/html", "text/xml",
  "application/json", "application/xml",
];
export const ACCEPTED_TYPES = [...IMAGE_TYPES, ...DOC_TYPES, ...TEXT_TYPES];

export function isImageType(type: string): boolean {
  return IMAGE_TYPES.includes(type);
}

export function isTextLikeType(type: string): boolean {
  return TEXT_TYPES.includes(type) || type.startsWith("text/");
}
