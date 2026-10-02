import { describe, expect, it } from 'vitest'

describe('JSDOM selector compatibility', () => {
  it('does not recurse through native fullscreen matching', () => {
    const button = document.createElement('button')
    document.body.append(button)
    try {
      expect(button.matches(':fullscreen')).toBe(false)
      expect(button.matches(':modal')).toBe(false)
      expect(window.getComputedStyle(button).display).not.toBe('none')
    } finally {
      button.remove()
    }
  })
})
