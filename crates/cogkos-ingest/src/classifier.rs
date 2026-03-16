/// Classification result
#[derive(Debug, Clone)]
pub struct CoarseClassification {
    pub entity_name: Option<String>,
    pub document_type: Option<String>,
    pub industry: Option<String>,
    pub keywords: Vec<String>,
}

/// Classify document based on filename and content
pub fn coarse_classify(filename: &str, _content_sample: &str) -> CoarseClassification {
    let lower = filename.to_lowercase();
    let mut result = CoarseClassification {
        entity_name: None,
        document_type: None,
        industry: None,
        keywords: Vec::new(),
    };

    // Extract entity name from filename patterns
    // Pattern: "XX公司2025年报.pdf" → Entity=XX公司
    if let Some(entity) = extract_entity_from_filename(&lower) {
        result.entity_name = Some(entity.clone());
        result.keywords.push(entity);
    }

    // Detect document type
    if lower.contains("年报") || lower.contains("annual") {
        result.document_type = Some("年报".to_string());
        result.keywords.push("年报".to_string());
    } else if lower.contains("季报") || lower.contains("quarterly") {
        result.document_type = Some("季报".to_string());
        result.keywords.push("季报".to_string());
    } else if lower.contains("报告") || lower.contains("report") {
        result.document_type = Some("报告".to_string());
        result.keywords.push("报告".to_string());
    } else if lower.contains("分析") || lower.contains("analysis") {
        result.document_type = Some("分析".to_string());
        result.keywords.push("分析".to_string());
    }

    // Detect industry (simple keyword matching)
    if lower.contains("制造") || lower.contains("manufacturing") {
        result.industry = Some("制造业".to_string());
    } else if lower.contains("零售") || lower.contains("retail") {
        result.industry = Some("零售业".to_string());
    } else if lower.contains("金融") || lower.contains("finance") {
        result.industry = Some("金融业".to_string());
    } else if lower.contains("科技") || lower.contains("tech") {
        result.industry = Some("科技业".to_string());
    }

    result
}

/// Extract entity name from filename
fn extract_entity_from_filename(filename: &str) -> Option<String> {
    // Remove extension
    let name = filename.split('.').next()?;

    // Try to extract Chinese company names (patterns like XX公司, XX集团)
    if let Some(pos) = name.find("公司") {
        return Some(name[..pos + 6].to_string()); // Include "公司"
    }
    if let Some(pos) = name.find("集团") {
        return Some(name[..pos + 6].to_string());
    }

    // Try English patterns (Company Name Year)
    let parts: Vec<&str> = name.split_whitespace().collect();
    if parts.len() >= 2 {
        // Return first part that looks like a name
        return Some(parts[0].to_string());
    }

    None
}

/// Extract initial node type from classification
pub fn node_type_from_classification(classification: &CoarseClassification) -> String {
    match classification.document_type.as_deref() {
        Some("年报") | Some("季报") => "File",
        Some("报告") => "File",
        Some("分析") => "Insight",
        _ => "File",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_annual_report() {
        let result = coarse_classify("华为2025年报.pdf", "");
        assert_eq!(result.document_type.as_deref(), Some("年报"));
    }

    #[test]
    fn classify_quarterly_report() {
        let result = coarse_classify("Q3季报.pdf", "");
        assert_eq!(result.document_type.as_deref(), Some("季报"));
    }

    #[test]
    fn classify_report() {
        let result = coarse_classify("行业报告.pdf", "");
        assert_eq!(result.document_type.as_deref(), Some("报告"));
    }

    #[test]
    fn classify_analysis() {
        let result = coarse_classify("市场分析.pdf", "");
        assert_eq!(result.document_type.as_deref(), Some("分析"));
    }

    #[test]
    fn classify_english_annual() {
        let result = coarse_classify("annual_report_2025.pdf", "");
        assert_eq!(result.document_type.as_deref(), Some("年报"));
    }

    #[test]
    fn classify_english_quarterly() {
        let result = coarse_classify("quarterly_results.pdf", "");
        assert_eq!(result.document_type.as_deref(), Some("季报"));
    }

    #[test]
    fn classify_manufacturing_industry() {
        let result = coarse_classify("制造业数据.pdf", "");
        assert_eq!(result.industry.as_deref(), Some("制造业"));
    }

    #[test]
    fn classify_retail_industry() {
        let result = coarse_classify("零售市场.pdf", "");
        assert_eq!(result.industry.as_deref(), Some("零售业"));
    }

    #[test]
    fn classify_finance_industry() {
        let result = coarse_classify("金融行业.pdf", "");
        assert_eq!(result.industry.as_deref(), Some("金融业"));
    }

    #[test]
    fn classify_tech_industry() {
        let result = coarse_classify("科技趋势.pdf", "");
        assert_eq!(result.industry.as_deref(), Some("科技业"));
    }

    #[test]
    fn classify_unknown_type() {
        let result = coarse_classify("data.csv", "");
        assert_eq!(result.document_type, None);
    }

    #[test]
    fn extract_entity_company() {
        let entity = extract_entity_from_filename("华为公司2025年报.pdf");
        assert!(entity.is_some());
        assert!(entity.unwrap().contains("公司"));
    }

    #[test]
    fn extract_entity_group() {
        let entity = extract_entity_from_filename("阿里集团报告.pdf");
        assert!(entity.is_some());
        assert!(entity.unwrap().contains("集团"));
    }

    #[test]
    fn extract_entity_english() {
        let entity = extract_entity_from_filename("Apple Report.pdf");
        assert!(entity.is_some());
    }

    #[test]
    fn extract_entity_none() {
        let entity = extract_entity_from_filename("data.csv");
        assert_eq!(entity, None);
    }

    #[test]
    fn node_type_annual_report() {
        let classification = CoarseClassification {
            entity_name: None,
            document_type: Some("年报".to_string()),
            industry: None,
            keywords: vec![],
        };
        assert_eq!(node_type_from_classification(&classification), "File");
    }

    #[test]
    fn node_type_analysis() {
        let classification = CoarseClassification {
            entity_name: None,
            document_type: Some("分析".to_string()),
            industry: None,
            keywords: vec![],
        };
        assert_eq!(node_type_from_classification(&classification), "Insight");
    }

    #[test]
    fn node_type_unknown() {
        let classification = CoarseClassification {
            entity_name: None,
            document_type: None,
            industry: None,
            keywords: vec![],
        };
        assert_eq!(node_type_from_classification(&classification), "File");
    }
}
