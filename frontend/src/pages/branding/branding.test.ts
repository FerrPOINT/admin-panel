import { describe, expect, it } from 'vitest'
import { readableForeground } from './index'

describe('branding preview contrast', () => {
  it('uses readable text on both default brand colors', () => {
    expect(readableForeground('#2563eb')).toBe('#ffffff')
    expect(readableForeground('#14b8a6')).toBe('#000000')
  })

  it('handles arbitrary light, dark and incomplete color input', () => {
    expect(readableForeground('#ffffff')).toBe('#000000')
    expect(readableForeground('#000000')).toBe('#ffffff')
    expect(readableForeground('#fff')).toBe('#000000')
  })
})
