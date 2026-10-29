import { describe, expect, it } from 'vitest';
import {
  NICK_COLOR_COUNT,
  formatIrcClock,
  memberRank,
  nickColorIndex,
  parseIrcInput,
  stripIrcFormatting,
} from './format';

describe('stripIrcFormatting', () => {
  it('drops bold/italic/underline codes but keeps the text', () => {
    expect(stripIrcFormatting('\u0002bold\u0002 and \u001ditalic\u001d')).toBe('bold and italic');
  });

  it('drops colour codes with foreground and background arguments', () => {
    expect(stripIrcFormatting('\u00034,2red on blue\u0003 plain')).toBe('red on blue plain');
    expect(stripIrcFormatting('\u000304shade\u0003')).toBe('shade');
    expect(stripIrcFormatting('no\u0003args')).toBe('noargs');
  });

  it('strips CTCP delimiters so actions never leak control characters', () => {
    expect(stripIrcFormatting('\u0001ACTION waves\u0001')).toBe('ACTION waves');
  });
});

describe('nickColorIndex', () => {
  it('is stable and case insensitive', () => {
    expect(nickColorIndex('Rosmontis')).toBe(nickColorIndex('rosmontis'));
  });

  it('stays inside the palette', () => {
    for (const nick of ['a', 'doctor', '迷迭香', 'someone_else', '12345']) {
      const index = nickColorIndex(nick);
      expect(index).toBeGreaterThanOrEqual(0);
      expect(index).toBeLessThan(NICK_COLOR_COUNT);
    }
  });

  it('spreads common nicks across buckets', () => {
    const buckets = new Set(['alice', 'bob', 'carol', 'dave'].map((nick) => nickColorIndex(nick)));
    expect(buckets.size).toBeGreaterThan(1);
  });
});

describe('parseIrcInput', () => {
  it('treats plain text as a channel message', () => {
    expect(parseIrcInput('  hello there  ', '#lorelibre_s2')).toEqual({ kind: 'message', text: 'hello there' });
  });

  it('maps /me to an action', () => {
    expect(parseIrcInput('/me waves', '#lorelibre_s2')).toEqual({ kind: 'action', text: 'waves' });
  });

  it('maps known slash commands to raw lines', () => {
    expect(parseIrcInput('/join #other', '#lorelibre_s2')).toEqual({ kind: 'raw', line: 'JOIN #other' });
    expect(parseIrcInput('/part bye', '#lorelibre_s2')).toEqual({ kind: 'raw', line: 'PART #lorelibre_s2 :bye' });
    expect(parseIrcInput('/nick doctor', '#lorelibre_s2')).toEqual({ kind: 'raw', line: 'NICK doctor' });
    expect(parseIrcInput('/topic hi all', '#lorelibre_s2')).toEqual({ kind: 'raw', line: 'TOPIC #lorelibre_s2 :hi all' });
    expect(parseIrcInput('/msg doctor hello', '#lorelibre_s2')).toEqual({ kind: 'raw', line: 'PRIVMSG doctor :hello' });
  });

  it('keeps unknown slash commands as messages instead of swallowing them', () => {
    expect(parseIrcInput('/shrug', '#lorelibre_s2')).toEqual({ kind: 'message', text: '/shrug' });
  });

  it('ignores empty input and commands missing arguments', () => {
    expect(parseIrcInput('   ', '#lorelibre_s2')).toEqual({ kind: 'empty' });
    expect(parseIrcInput('/me', '#lorelibre_s2')).toEqual({ kind: 'empty' });
    expect(parseIrcInput('/join', '#lorelibre_s2')).toEqual({ kind: 'empty' });
  });
});

describe('memberRank', () => {
  it('orders operators before voiced members before everyone else', () => {
    expect(memberRank('@')).toBeLessThan(memberRank('+'));
    expect(memberRank('+')).toBeLessThan(memberRank(''));
    expect(memberRank('~')).toBeLessThan(memberRank('&'));
  });
});

describe('formatIrcClock', () => {
  it('returns an empty string for missing timestamps', () => {
    expect(formatIrcClock(0)).toBe('');
    expect(formatIrcClock(Number.NaN)).toBe('');
  });

  it('formats a valid timestamp', () => {
    expect(formatIrcClock(Date.UTC(2026, 9, 29, 12, 5))).toMatch(/\d/);
  });
});
