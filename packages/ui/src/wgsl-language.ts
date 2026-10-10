import { StreamLanguage } from '@codemirror/language';

const KEYWORDS = new Set([
  'alias', 'break', 'case', 'const', 'const_assert', 'continue', 'continuing', 'default', 'diagnostic', 'discard',
  'else', 'enable', 'fn', 'for', 'if', 'let', 'loop', 'override', 'requires', 'return', 'struct', 'switch', 'var', 'while',
]);
const TYPES = /^(bool|f16|f32|i32|u32|vec[234][fhiu]?|mat[234]x[234][fh]?|array|atomic|ptr|sampler(_comparison)?|texture_\w+)$/;
const ATOMS = new Set(['true', 'false']);

/** WGSL highlighting good enough to read by: keywords, types, attributes, numbers, comments. */
export const wgsl = StreamLanguage.define<{ comment: number }>({
  name: 'wgsl',
  startState: () => ({ comment: 0 }),
  token(stream, state) {
    if (state.comment > 0) {
      while (!stream.eol()) {
        if (stream.match('/*')) state.comment++;
        else if (stream.match('*/')) {
          state.comment--;
          if (state.comment === 0) break;
        } else stream.next();
      }
      return 'comment';
    }
    if (stream.eatSpace()) return null;
    if (stream.match('//')) {
      stream.skipToEnd();
      return 'comment';
    }
    if (stream.match('/*')) {
      state.comment = 1;
      return 'comment';
    }
    if (stream.match(/^@\s*\w+/)) return 'meta';
    if (stream.match(/^(0[xX][0-9a-fA-F]+|(\d+\.\d*|\.\d+|\d+)([eE][+-]?\d+)?)[iufh]?/)) return 'number';
    const word = stream.match(/^[A-Za-z_]\w*/) as RegExpMatchArray | null;
    if (word) {
      const text = word[0];
      if (KEYWORDS.has(text)) return 'keyword';
      if (ATOMS.has(text)) return 'atom';
      if (TYPES.test(text)) return 'typeName';
      if (stream.match(/^\s*\(/, false)) return 'variableName.function';
      return 'variableName';
    }
    stream.next();
    return null;
  },
  languageData: { commentTokens: { line: '//', block: { open: '/*', close: '*/' } } },
});
