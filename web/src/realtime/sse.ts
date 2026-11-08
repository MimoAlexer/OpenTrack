export type Frame = { event: string; id: string; data: string };
const MAX_FRAME = 64 * 1024;
/** Incremental SSE parser. Never retain a complete response or an unbounded line. */
export class SseParser {
  private line = ''; private event = ''; private id = ''; private data: string[] = [];
  private size = 0; private carriageReturn = false; private first = true;
  constructor(private emit: (frame: Frame) => void) {}
  push(chunk: string) {
    for (const character of chunk) {
      if (this.first) { this.first = false; if (character === '\uFEFF') continue; }
      if (this.carriageReturn) { this.carriageReturn = false; if (character === '\n') continue; }
      const point = character.codePointAt(0)!; this.size += point <= 0x7f ? 1 : point <= 0x7ff ? 2 : point <= 0xffff ? 3 : 4;
      if (this.size > MAX_FRAME) throw new Error('Realtime frame exceeds its limit.');
      if (character === '\r' || character === '\n') { this.carriageReturn = character === '\r'; this.finishLine(); }
      else this.line += character;
    }
  }
  private finishLine() {
    const line = this.line; this.line = '';
    if (!line) {
      const frame = { event: this.event, id: this.id, data: this.data.join('\n') };
      this.event = ''; this.id = ''; this.data = []; this.size = 0;
      if (frame.data) this.emit(frame);
      return;
    }
    if (line.startsWith(':')) return;
    const separator = line.indexOf(':'); const field = separator < 0 ? line : line.slice(0, separator);
    let value = separator < 0 ? '' : line.slice(separator + 1); if (value.startsWith(' ')) value = value.slice(1);
    if (field === 'event') this.event = value;
    if (field === 'id') { if (value.includes('\0')) throw new Error('Invalid realtime cursor.'); this.id = value; }
    if (field === 'data') this.data.push(value);
  }
}
