/** Frame dimensions encoded in an avash string. */
export declare function dimensions(hash: string): { width: number; height: number };

/** Rebuild a standalone AVIF file from an avash string. */
export declare function toAvif(hash: string): Uint8Array;

/** Object URL for the placeholder image. Caller revokes. */
export declare function toObjectURL(hash: string): string;

/** Decode to an ImageBitmap at native (tiny) size. */
export declare function decode(hash: string): Promise<ImageBitmap>;

/** Show the avash as a placeholder behind an `<img avash="...">` until its real source loads. A failed load keeps it. */
export declare function apply(img: HTMLImageElement): void;

/** Apply to every `img[avash]` under root and keep watching for new ones. */
export declare function observe(root?: ParentNode): void;
