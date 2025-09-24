use server::auth::TokenIssuer;
use server::config::Config;
use server::matchmaker::Matchmaker;
use server::protocol::RoomSeed;

#[tokio::test]
async fn resume_token_round_trip() {
    let config = Config::default();
    let issuer = TokenIssuer::new(config.token_secret.clone());
    let matchmaker = Matchmaker::new(config.clone(), issuer.clone());
    let bootstrap = matchmaker.create_room().unwrap();
    let resume = issuer.mint_resume_token(&bootstrap.room_id, &bootstrap.player_id);
    matchmaker
        .set_resume_token(&bootstrap.room_id, &bootstrap.player_id, resume.0.clone())
        .await;
    let valid = matchmaker
        .validate_resume_token(&bootstrap.room_id, &bootstrap.player_id, &resume.0)
        .await;
    assert!(valid);
}
