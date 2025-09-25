use server::auth::TokenIssuer;
use server::config::Config;
use server::matchmaker::{CreateRoomRequest, Matchmaker};

#[tokio::test]
async fn resume_token_round_trip() {
    let config = Config::default();
    let issuer = TokenIssuer::new(config.token_secret.clone());
    let matchmaker = Matchmaker::new(config.clone(), issuer.clone());
    let bootstrap = matchmaker
        .create_room(CreateRoomRequest {
            name: "host".to_string(),
            region: config.region.clone(),
            max_players: 2,
            mode: "endless".to_string(),
        })
        .await
        .unwrap();
    let claims = issuer.verify_ws_token(&bootstrap.ws_token).unwrap();
    let resume = issuer.mint_resume_token(&bootstrap.room_id, &claims.player_id);
    matchmaker
        .set_resume_token(&bootstrap.room_id, &claims.player_id, resume.0.clone())
        .await;
    let valid = matchmaker
        .validate_resume_token(&bootstrap.room_id, &claims.player_id, &resume.0)
        .await;
    assert!(valid);
}
