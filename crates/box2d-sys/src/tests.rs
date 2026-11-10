use super::*;

#[test]
fn links_and_simulates() {
    unsafe {
        let mut world_def = b2DefaultWorldDef();
        world_def.gravity = b2Vec2 { x: 0.0, y: -10.0 };
        let world = b2CreateWorld(&world_def);
        assert!(b2World_IsValid(world));

        let mut body_def = b2DefaultBodyDef();
        body_def.type_ = b2BodyType_b2_dynamicBody;
        body_def.position = b2Vec2 { x: 0.0, y: 10.0 };
        let body = b2CreateBody(world, &body_def);
        let shape_def = b2DefaultShapeDef();
        b2CreateCircleShape(body, &shape_def, &b2Circle { center: b2Vec2 { x: 0.0, y: 0.0 }, radius: 0.5 });

        for _ in 0..60 {
            b2World_Step(world, 1.0 / 60.0, 4);
        }
        let p = b2Body_GetPosition(body);
        assert!(p.y < 6.0 && p.y > 4.0, "free fall for 1 s from y=10 ends near y=5, got {}", p.y);

        let hit = b2World_CastRayClosest(world, b2Vec2 { x: -5.0, y: p.y }, b2Vec2 { x: 10.0, y: 0.0 }, b2DefaultQueryFilter());
        assert!(hit.hit);
        b2DestroyWorld(world);
        assert!(!b2World_IsValid(world));
    }
}
