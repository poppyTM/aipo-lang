//! ECS World state for Poppy simulations.

use aipo_host::{Handle, HandleTable, HostFault};

use crate::commands::{CommandBuffer, PoppyCommand};

/// Internal record of a live game entity.
#[derive(Debug, Clone, PartialEq)]
pub struct EntityRecord {
    /// Identifying tag or archetype classification.
    pub tag: String,
    /// X coordinate in world space.
    pub x: f64,
    /// Y coordinate in world space.
    pub y: f64,
    /// Velocity in X direction (units/second).
    pub vx: f64,
    /// Velocity in Y direction (units/second).
    pub vy: f64,
    /// Whether the entity is marked for deletion.
    pub marked_for_despawn: bool,
}

/// The ECS World owning entities, components, and deferred command buffer.
#[derive(Debug, Default)]
pub struct World {
    entities: HandleTable<EntityRecord>,
    commands: CommandBuffer,
}

impl World {
    /// Creates a fresh, empty world.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entities: HandleTable::new(),
            commands: CommandBuffer::new(),
        }
    }

    /// Allocates an entity handle immediately and inserts it into the table.
    pub fn spawn_immediate(&mut self, tag: &str, x: f64, y: f64, vx: f64, vy: f64) -> Handle {
        self.entities.insert(EntityRecord {
            tag: tag.to_string(),
            x,
            y,
            vx,
            vy,
            marked_for_despawn: false,
        })
    }

    /// Queues an entity spawn command with an allocated handle.
    pub fn queue_spawn(&mut self, tag: &str, x: f64, y: f64, vx: f64, vy: f64) -> Handle {
        // Allocate slot now so caller receives a valid generational handle.
        let handle = self.entities.insert(EntityRecord {
            tag: tag.to_string(),
            x,
            y,
            vx,
            vy,
            marked_for_despawn: false,
        });
        self.commands.push(PoppyCommand::Spawn {
            handle,
            tag: tag.to_string(),
            x,
            y,
            vx,
            vy,
        });
        handle
    }

    /// Queues an entity despawn command.
    ///
    /// # Errors
    /// Returns [`HostFault::StaleHandle`] if the handle is not live.
    pub fn queue_despawn(&mut self, handle: Handle) -> Result<(), HostFault> {
        let record = self
            .entities
            .get_mut(handle)
            .ok_or(HostFault::StaleHandle { handle })?;
        record.marked_for_despawn = true;
        self.commands.push(PoppyCommand::Despawn { handle });
        Ok(())
    }

    /// Resolves an entity's data or returns [`HostFault::StaleHandle`] if stale.
    ///
    /// # Errors
    /// Returns [`HostFault::StaleHandle`] if the handle is stale or despawned.
    pub fn get_entity(&self, handle: Handle) -> Result<&EntityRecord, HostFault> {
        let record = self
            .entities
            .get(handle)
            .ok_or(HostFault::StaleHandle { handle })?;
        if record.marked_for_despawn {
            return Err(HostFault::StaleHandle { handle });
        }
        Ok(record)
    }

    /// Resolves mutable entity data or returns [`HostFault::StaleHandle`] if stale.
    ///
    /// # Errors
    /// Returns [`HostFault::StaleHandle`] if the handle is stale or despawned.
    pub fn get_entity_mut(&mut self, handle: Handle) -> Result<&mut EntityRecord, HostFault> {
        let record = self
            .entities
            .get_mut(handle)
            .ok_or(HostFault::StaleHandle { handle })?;
        if record.marked_for_despawn {
            return Err(HostFault::StaleHandle { handle });
        }
        Ok(record)
    }

    /// Queries all active, non-despawned entities matching a tag.
    pub fn query(&self, tag: &str) -> Vec<Handle> {
        let mut matching = Vec::new();
        for (handle, record) in self.entities.iter() {
            if !record.marked_for_despawn && record.tag == tag {
                matching.push(handle);
            }
        }
        // Deterministic sorting by handle index
        matching.sort_by_key(|h| (h.index(), h.generation()));
        matching
    }

    /// Access the deferred command buffer.
    pub fn commands(&mut self) -> &mut CommandBuffer {
        &mut self.commands
    }

    /// Count of currently live entities.
    #[must_use]
    pub fn live_count(&self) -> usize {
        self.entities
            .iter()
            .filter(|(_, r)| !r.marked_for_despawn)
            .count()
    }

    /// Applies all queued commands at a safe point.
    pub fn apply_deferred(&mut self) {
        let cmds = self.commands.drain();
        for cmd in cmds {
            match cmd {
                PoppyCommand::Spawn {
                    handle,
                    tag,
                    x,
                    y,
                    vx,
                    vy,
                } => {
                    if let Some(record) = self.entities.get_mut(handle) {
                        record.tag = tag;
                        record.x = x;
                        record.y = y;
                        record.vx = vx;
                        record.vy = vy;
                        record.marked_for_despawn = false;
                    }
                }
                PoppyCommand::Despawn { handle } => {
                    let _ = self.entities.remove(handle);
                }
                PoppyCommand::SetPosition { handle, x, y } => {
                    if let Some(record) = self.entities.get_mut(handle)
                        && !record.marked_for_despawn
                    {
                        record.x = x;
                        record.y = y;
                    }
                }
                PoppyCommand::SetVelocity { handle, vx, vy } => {
                    if let Some(record) = self.entities.get_mut(handle)
                        && !record.marked_for_despawn
                    {
                        record.vx = vx;
                        record.vy = vy;
                    }
                }
            }
        }
    }

    /// Advances physics positions of all live entities by `dt`.
    pub fn integrate(&mut self, dt: f64) {
        let handles: Vec<Handle> = self
            .entities
            .iter()
            .filter(|(_, r)| !r.marked_for_despawn)
            .map(|(h, _)| h)
            .collect();
        for h in handles {
            if let Some(r) = self.entities.get_mut(h) {
                r.x += r.vx * dt;
                r.y += r.vy * dt;
            }
        }
    }

    /// Computes a deterministic 64-bit FNV-1a state digest of the world.
    #[must_use]
    pub fn digest(&self, tick: u64, rng_state: u64) -> u64 {
        const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

        fn feed_u64(hash: &mut u64, val: u64) {
            const FNV_PRIME: u64 = 0x0100_0000_01b3;
            for b in val.to_le_bytes() {
                *hash ^= u64::from(b);
                *hash = hash.wrapping_mul(FNV_PRIME);
            }
        }

        fn feed_bytes(hash: &mut u64, bytes: &[u8]) {
            const FNV_PRIME: u64 = 0x0100_0000_01b3;
            for b in bytes {
                *hash ^= u64::from(*b);
                *hash = hash.wrapping_mul(FNV_PRIME);
            }
        }

        let mut hash = FNV_OFFSET;

        feed_u64(&mut hash, tick);
        feed_u64(&mut hash, rng_state);

        // Sort entities by index for determinism
        let mut records: Vec<(Handle, &EntityRecord)> = self
            .entities
            .iter()
            .filter(|(_, r)| !r.marked_for_despawn)
            .collect();
        records.sort_by_key(|(h, _)| (h.index(), h.generation()));

        for (handle, record) in records {
            feed_u64(&mut hash, handle.index() as u64);
            feed_u64(&mut hash, u64::from(handle.generation()));
            feed_bytes(&mut hash, record.tag.as_bytes());
            feed_u64(&mut hash, record.x.to_bits());
            feed_u64(&mut hash, record.y.to_bits());
            feed_u64(&mut hash, record.vx.to_bits());
            feed_u64(&mut hash, record.vy.to_bits());
        }

        hash
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_world_spawn_and_query() {
        let mut world = World::new();
        let e1 = world.spawn_immediate("player", 0.0, 0.0, 1.0, 0.0);
        let e2 = world.spawn_immediate("enemy", 10.0, 10.0, 0.0, 0.0);
        let e3 = world.spawn_immediate("enemy", 20.0, 20.0, -1.0, 0.0);

        let enemies = world.query("enemy");
        assert_eq!(enemies.len(), 2);
        assert!(enemies.contains(&e2));
        assert!(enemies.contains(&e3));

        let players = world.query("player");
        assert_eq!(players, vec![e1]);
    }

    #[test]
    fn test_world_despawn_deferred() {
        let mut world = World::new();
        let e1 = world.spawn_immediate("asteroid", 5.0, 5.0, 0.0, 0.0);
        assert_eq!(world.live_count(), 1);

        world.queue_despawn(e1).expect("valid handle");
        // Marked for despawn immediately hides it from query
        assert_eq!(world.query("asteroid").len(), 0);

        // But slot is not physically released until safe point
        world.apply_deferred();
        assert_eq!(world.live_count(), 0);

        // Post-release, handle is stale
        assert!(world.get_entity(e1).is_err());
    }

    #[test]
    fn test_world_digest_determinism() {
        let mut w1 = World::new();
        let mut w2 = World::new();

        w1.spawn_immediate("bullet", 1.5, 2.5, 10.0, 0.0);
        w2.spawn_immediate("bullet", 1.5, 2.5, 10.0, 0.0);

        let d1 = w1.digest(10, 999);
        let d2 = w2.digest(10, 999);
        assert_eq!(d1, d2);

        w1.integrate(0.1);
        w2.integrate(0.1);

        let d1_next = w1.digest(11, 1000);
        let d2_next = w2.digest(11, 1000);
        assert_eq!(d1_next, d2_next);
        assert_ne!(d1, d1_next);
    }
}
