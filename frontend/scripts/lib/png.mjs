import zlib from "node:zlib";

/**
 * 最小 PNG 解码器（支持 8-bit RGB 与 RGBA，非隔行）。
 * @param {Buffer} buf
 * @returns {{ width: number, height: number, channels: number, data: Buffer } | null}
 */
export function decodePng(buf) {
  try {
    if (!buf || buf.length < 8) return null;
    if (!(buf[0] === 0x89 && buf.toString("ascii", 1, 4) === "PNG")) return null;
    let pos = 8;
    let w = 0;
    let h = 0;
    let colorType = 0;
    let bitDepth = 0;
    const idat = [];
    while (pos < buf.length) {
      const len = buf.readUInt32BE(pos);
      const type = buf.toString("ascii", pos + 4, pos + 8);
      const data = buf.subarray(pos + 8, pos + 8 + len);
      if (type === "IHDR") {
        w = data.readUInt32BE(0);
        h = data.readUInt32BE(4);
        bitDepth = data[8];
        colorType = data[9];
        if (data[12] !== 0 || bitDepth !== 8 || (colorType !== 6 && colorType !== 2)) return null;
      } else if (type === "IDAT") {
        idat.push(data);
      } else if (type === "IEND") {
        break;
      }
      pos += 12 + len;
    }
    if (w === 0 || h === 0 || idat.length === 0) return null;
    const raw = zlib.inflateSync(Buffer.concat(idat));
    const channels = colorType === 6 ? 4 : 3;
    const stride = w * channels;
    if (raw.length < h * (stride + 1)) return null;

    const data = Buffer.alloc(w * h * channels);
    let prev = Buffer.alloc(stride);

    const paeth = (a, b, c) => {
      const pa = Math.abs(b - c);
      const pb = Math.abs(a - c);
      const pc = Math.abs(a + b - 2 * c);
      return pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
    };

    for (let y = 0; y < h; y++) {
      const rawOffset = y * (stride + 1);
      const filter = raw[rawOffset];
      const cur = data.subarray(y * stride, (y + 1) * stride);
      for (let x = 0; x < stride; x++) {
        const rawByte = raw[rawOffset + 1 + x];
        const a = x >= channels ? cur[x - channels] : 0;
        const b = prev[x];
        const c = x >= channels ? prev[x - channels] : 0;
        let v = rawByte;
        if (filter === 1) v = (v + a) & 0xff;
        else if (filter === 2) v = (v + b) & 0xff;
        else if (filter === 3) v = (v + ((a + b) >> 1)) & 0xff;
        else if (filter === 4) v = (v + paeth(a, b, c)) & 0xff;
        cur[x] = v;
      }
      prev = cur;
    }

    return { width: w, height: h, channels, data };
  } catch {
    return null;
  }
}
