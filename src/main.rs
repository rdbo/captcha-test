use actix_files::NamedFile;
use actix_web::{HttpServer, App, get, post, web, Responder};
use chrono::{Timelike, NaiveDateTime, Utc};
use uuid::Uuid;
use captcha::Captcha;
use captcha::filters::{Wave, Noise};
use serde::{Serialize, Deserialize};
use rand::{Rng, thread_rng};
use deadpool_postgres::{Pool, Config};
use tokio_postgres::NoTls;

#[get("/")]
async fn index() -> actix_web::Result<NamedFile> {
    Ok(NamedFile::open("static/index.html")?)
}

#[derive(Deserialize)]
struct IndexForm {
    name : String,
    email : String,
    captcha_id : String,
    captcha_answer : String
}

#[post("/submit")]
async fn submit(form : web::Form<IndexForm>, db_pool : web::Data<Pool>) -> impl Responder {
    let base_response = format!(concat!(
        "Name: {}\n",
        "E-Mail: {}\n",
        "Captcha ID: {}\n",
        "Captcha Answer: {}\n",
        "Captcha Solved? : "),
        form.name, form.email, form.captcha_id, form.captcha_answer
    );
    println!("Base Response: {}", base_response);
    let mut response : String = format!("Invalid response");
    let pgclient = db_pool.get().await.unwrap();
    let captcha_id = Uuid::parse_str(&form.captcha_id).unwrap();
    for row in pgclient.query("SELECT answer FROM captcha WHERE id = $1", &[&captcha_id]).await.unwrap() {
        let correct_answer : String = row.get(0);
        println!("Correct Answer: {}", correct_answer);
        response = format!("{} {}\nCorrect Answer: {}", base_response, if correct_answer == form.captcha_answer { "yes" } else { "no" }, correct_answer);
        // TODO: Delete captcha after attempt, improve code (no need for a loop when there's only one result)
    }
    return response;
}

#[get("/jquery.js")]
async fn jquery() -> actix_web::Result<NamedFile> {
    Ok(NamedFile::open("static/jquery-3.6.0.min.js")?)
}

#[get("/api/captcha")]
async fn api_captcha(db_pool : web::Data<Pool>) -> impl Responder {
    #[derive(Serialize)]
    struct CaptchaData {
        id : String,
        image : String
    }

    let pgclient = db_pool.get().await.unwrap();

    let rand_distort = || (thread_rng().gen_range(1..5) as f64, thread_rng().gen_range(5..20) as f64);
    let hdistort = rand_distort();
    let vdistort = rand_distort();
    let noise = thread_rng().gen_range(2..8) as f32 / 10.0;
    let mut cap = Captcha::new();
    cap.add_chars(thread_rng().gen_range(5..8))
        .apply_filter(Wave::new(hdistort.0, hdistort.1).horizontal())
        .apply_filter(Wave::new(vdistort.0, vdistort.1).vertical())
        .apply_filter(Noise::new(noise))
        .view(280, 280);
    let img = match cap.as_base64() {
        Some(data) => data,
        None => return web::Json(r#"{ "error" : "Unable to generate captcha" }"#.to_string())
    };

    let now = Utc::now();
    let creation = NaiveDateTime::from_timestamp(now.timestamp(), now.nanosecond());
    let answer = cap.chars_as_string();
    let id = Uuid::new_v4();

    pgclient.execute("INSERT INTO captcha(id, answer, creation) VALUES($1, $2, $3)", &[&id, &answer, &creation]).await.unwrap();

    let data = CaptchaData {
        id : id.to_string(),
        image : img
    };

    let json = match serde_json::to_string(&data) {
        Ok(s) => s,
        Err(_) => r#"{ "error" : "Unable parse captcha data" }"#.to_string()
    };

    web::Json(json)
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let mut config = Config::new();
    config.host = Some("localhost".to_string());
    config.user = Some("_postgresql".to_string());
    config.dbname = Some("captcha-test".to_string());
    let pool = config.create_pool(None, NoTls).unwrap();
    HttpServer::new(move || { 
        App::new()
        .app_data(web::Data::new(pool.clone()))
        .service(index)
        .service(submit)
        .service(jquery)
        .service(api_captcha)
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}
