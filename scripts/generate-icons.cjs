// Run with Node.js and sharp available (npm install --no-save sharp).
// All application icons derive from the same SVG used by the frontend.
const fs = require("node:fs/promises");
const path = require("node:path");
const sharp = require("sharp");

async function main() {
  const root = path.resolve(__dirname, "..");
  const source = path.join(root, "frontend/public/logo.svg");
  const sizes = [16, 24, 32, 48, 64, 128, 256];
  const images = await Promise.all(
    sizes.map(async (size) => {
      const raster = sharp(source).resize(size, size);
      if (size === 256) return raster.png().toBuffer();
      // Small sizes use DIB entries for Windows resource compilers and NSIS.
      const rgba = await raster.ensureAlpha().raw().toBuffer();
      const maskStride = Math.ceil(size / 32) * 4;
      const dib = Buffer.alloc(40 + size * size * 4 + maskStride * size);
      dib.writeUInt32LE(40, 0);
      dib.writeInt32LE(size, 4);
      dib.writeInt32LE(size * 2, 8);
      dib.writeUInt16LE(1, 12);
      dib.writeUInt16LE(32, 14);
      dib.writeUInt32LE(size * size * 4, 20);
      for (let y = 0; y < size; y++) {
        for (let x = 0; x < size; x++) {
          const src = (y * size + x) * 4;
          const dest = 40 + ((size - 1 - y) * size + x) * 4;
          dib[dest] = rgba[src + 2];
          dib[dest + 1] = rgba[src + 1];
          dib[dest + 2] = rgba[src];
          dib[dest + 3] = rgba[src + 3];
          if (rgba[src + 3] === 0) {
            const mask = 40 + size * size * 4 + (size - 1 - y) * maskStride;
            dib[mask + Math.floor(x / 8)] |= 0x80 >> (x % 8);
          }
        }
      }
      return dib;
    }),
  );
  const header = Buffer.alloc(6 + 16 * sizes.length);
  header.writeUInt16LE(1, 2);
  header.writeUInt16LE(sizes.length, 4);
  let offset = header.length;
  images.forEach((image, index) => {
    const entry = 6 + index * 16;
    header[entry] = sizes[index] % 256;
    header[entry + 1] = sizes[index] % 256;
    header.writeUInt16LE(1, entry + 4);
    header.writeUInt16LE(32, entry + 6);
    header.writeUInt32LE(image.length, entry + 8);
    header.writeUInt32LE(offset, entry + 12);
    offset += image.length;
  });
  await fs.writeFile(
    path.join(root, "crates/app/icons/icon.ico"),
    Buffer.concat([header, ...images]),
  );
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
