import { describe, expect, it } from 'vitest'
import { contextTokens, dollarAmount, modelContextText, type Draft } from './ai'

describe('exact budget amount display', () => {
  it('preserves microdollars and does not round a large receipt through Number', () => {
    expect(dollarAmount('30000000')).toBe('$30.00')
    expect(dollarAmount('0')).toBe('$0.00')
    expect(dollarAmount('1')).toBe('$0.000001')
    expect(dollarAmount('18446744073709551615')).toBe('$18446744073709.551615')
    for (const value of ['-1', 'NaN', '1e6', '01', '1.1', '18446744073709551616']) {
      expect(dollarAmount(value)).toBe('Недоступно')
    }
  })
})

describe('AI context in thousands of tokens', () => {
  it('uses exact decimal thousands and rejects ambiguous or unsafe budgets', () => {
    expect(contextTokens('256')).toBe(256000)
    expect(contextTokens('64')).toBe(64000)
    for (const value of ['', '0', '63', '256.1', '2e3', '-256', 'Infinity', '4294968']) {
      expect(contextTokens(value)).toBeNull()
    }
  })
})

describe('model context restoration', () => {
  const draft: Draft = {
    settings: { provider: 'openrouter', model: 'selected', context_window_tokens: 128000 },
    draft_revision: 4,
    updated_at: '',
    model_contexts: [{ model: 'remembered', context_window_tokens: 192000 }],
  }
  it('restores saved model budgets and defaults new models to 256K', () => {
    expect(modelContextText(draft, 'selected')).toBe('128')
    expect(modelContextText(draft, 'remembered')).toBe('192')
    expect(modelContextText(draft, 'new')).toBe('256')
    expect(modelContextText(undefined, 'remembered')).toBe('256')
  })
  it('preserves unsaved text, including invalid and empty drafts, across model switches', () => {
    const pending = { remembered: '', selected: '63', new: '512' }
    expect(modelContextText(draft, 'remembered', pending)).toBe('')
    expect(modelContextText(draft, 'new', pending)).toBe('512')
    expect(modelContextText(draft, 'selected', pending)).toBe('63')
    expect(contextTokens(modelContextText(draft, 'selected', pending))).toBeNull()
    expect(modelContextText(draft, 'remembered')).toBe('192')
  })
})
