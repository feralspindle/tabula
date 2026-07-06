// parity contract with tabula-core::apply (kernel). same closed vocabulary,
// same no-op semantics for ill-formed deltas, if these diverge, client
// projections drift from the server's.
import { describe, it, expect } from 'vitest'
import { applyDelta, applyRecord } from './fold'

const E1 = '01890000-0000-7000-8000-000000000001'
const E2 = '01890000-0000-7000-8000-000000000002'

describe('applyDelta', () => {
  it('spawn creates an empty entity, idempotently', () => {
    const world = {}
    applyDelta(world, { op: 'spawn', entity: E1 })
    expect(world[E1]).toEqual({})
    world[E1]['core.name'] = 'Keep me'
    applyDelta(world, { op: 'spawn', entity: E1 })
    expect(world[E1]['core.name']).toBe('Keep me') // never clobbers
  })

  it('set writes only to spawned entities', () => {
    const world = {}
    applyDelta(world, { op: 'set', entity: E1, component: 'a.b', value: 1 })
    expect(world).toEqual({}) // no implicit spawn

    applyDelta(world, { op: 'spawn', entity: E1 })
    applyDelta(world, { op: 'set', entity: E1, component: 'a.b', value: 1 })
    expect(world[E1]['a.b']).toBe(1)
  })

  it('remove and despawn', () => {
    const world = { [E1]: { 'a.b': 1, 'a.c': 2 }, [E2]: {} }
    applyDelta(world, { op: 'remove', entity: E1, component: 'a.b' })
    expect(world[E1]).toEqual({ 'a.c': 2 })
    applyDelta(world, { op: 'despawn', entity: E1 })
    expect(E1 in world).toBe(false)
    expect(E2 in world).toBe(true)
  })

  it('applyRecord folds deltas in order', () => {
    const world = {}
    applyRecord(world, {
      seq: 1,
      deltas: [
        { op: 'spawn', entity: E1 },
        { op: 'set', entity: E1, component: 'counter.value', value: 0 },
        { op: 'set', entity: E1, component: 'counter.value', value: 5 },
      ],
    })
    expect(world[E1]['counter.value']).toBe(5)
  })
})
