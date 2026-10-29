use super::{HttpTranslator, json_response, official::google_code};
use crate::{AppError, ErrorCode, Result, TranslatedItem, TranslationRequest};
use tokio_util::sync::CancellationToken;

pub async fn google(
    backend: &HttpTranslator,
    input: &TranslationRequest,
    stop: &CancellationToken,
) -> Result<Vec<TranslatedItem>> {
    let mut output = Vec::with_capacity(input.items.len());
    for item in &input.items {
        let mut text = String::new();
        // Keep requests within the unofficial endpoint's practical URL limit.
        let characters: Vec<_> = item.text.chars().collect();
        for segment in characters.chunks(1200) {
            let segment: String = segment.iter().collect();
            let request = backend
                .client
                .get("https://translate.googleapis.com/translate_a/single")
                .query(&[
                    ("client", "gtx"),
                    ("sl", google_code(&input.source)),
                    ("tl", google_code(&input.target)),
                    ("dt", "t"),
                    ("q", &segment),
                ]);
            let result = json_response(request, stop).await?;
            let pieces = result.get(0).and_then(|v| v.as_array()).ok_or_else(|| {
                AppError::new(
                    ErrorCode::InvalidResponse,
                    "Google 网页接口格式已变化，请切换其他服务",
                )
            })?;
            for piece in pieces {
                if let Some(piece) = piece.get(0).and_then(|v| v.as_str()) {
                    text.push_str(piece);
                }
            }
        }
        output.push(TranslatedItem {
            id: item.id.clone(),
            text,
        });
    }
    Ok(output)
}
