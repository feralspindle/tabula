// client-side mirror of the kernel's game-agnostic fold (tabula-core::apply).
// MUST stay semantically identical: same closed vocabulary, same no-op
// behavior for ill-formed deltas. replay never executes plugin code here
// either, the client is just another projection.

export function applyDelta(world, delta) {
  switch (delta.op) {
    case 'spawn':
      if (!(delta.entity in world)) world[delta.entity] = {}
      break
    case 'despawn':
      delete world[delta.entity]
      break
    case 'set':
      if (world[delta.entity]) world[delta.entity][delta.component] = delta.value
      break
    case 'remove':
      if (world[delta.entity]) delete world[delta.entity][delta.component]
      break
  }
}

export function applyRecord(world, record) {
  for (const delta of record.deltas) applyDelta(world, delta)
}
