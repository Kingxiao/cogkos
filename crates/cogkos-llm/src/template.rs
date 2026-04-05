use crate::error::{LlmError, Result};
use crate::types::Message;
use regex::Regex;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct PromptTemplate {
    pub id: String,
    pub template: String,
    pub description: String,
    pub variables: Vec<String>,
    pub default_values: HashMap<String, String>,
}

impl PromptTemplate {
    pub fn new(id: impl Into<String>, template: impl Into<String>) -> Result<Self> {
        let template_str = template.into();
        let variables = Self::extract_variables(&template_str)?;

        Ok(Self {
            id: id.into(),
            template: template_str,
            description: String::new(),
            variables,
            default_values: HashMap::new(),
        })
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    pub fn with_defaults(mut self, defaults: HashMap<String, String>) -> Self {
        self.default_values = defaults;
        self
    }

    pub fn render(&self, variables: &HashMap<String, String>) -> Result<String> {
        let mut result = self.template.clone();

        for var in &self.variables {
            let value = variables
                .get(var)
                .or_else(|| self.default_values.get(var))
                .ok_or_else(|| LlmError::TemplateError(format!("Missing variable: {}", var)))?;

            let placeholder = format!("{{{{{}}}}}", var);
            result = result.replace(&placeholder, value);
        }

        Ok(result)
    }

    fn extract_variables(template: &str) -> Result<Vec<String>> {
        let re = Regex::new(r"\{\{(\w+)\}\}")
            .map_err(|e| LlmError::TemplateError(format!("Invalid regex: {}", e)))?;

        let mut variables = Vec::new();
        for cap in re.captures_iter(template) {
            if let Some(m) = cap.get(1) {
                let var = m.as_str().to_string();
                if !variables.contains(&var) {
                    variables.push(var);
                }
            }
        }

        Ok(variables)
    }
}

pub struct TemplateManager {
    templates: HashMap<String, PromptTemplate>,
}

impl Default for TemplateManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TemplateManager {
    pub fn new() -> Self {
        let mut manager = Self {
            templates: HashMap::new(),
        };

        manager.register_default_templates();
        manager
    }

    pub fn register(&mut self, template: PromptTemplate) {
        self.templates.insert(template.id.clone(), template);
    }

    pub fn get(&self, id: &str) -> Option<&PromptTemplate> {
        self.templates.get(id)
    }

    pub fn remove(&mut self, id: &str) -> Option<PromptTemplate> {
        self.templates.remove(id)
    }

    pub fn list(&self) -> Vec<&PromptTemplate> {
        self.templates.values().collect()
    }

    pub fn render(&self, template_id: &str, variables: &HashMap<String, String>) -> Result<String> {
        let template = self.get(template_id).ok_or_else(|| {
            LlmError::TemplateError(format!("Template not found: {}", template_id))
        })?;
        template.render(variables)
    }

    fn register_default_templates(&mut self) {
        // Query analysis template
        let query_analysis = PromptTemplate::new(
            "query_analysis",
            r#"Analyze the following query and extract key information:

Query: {{query}}
Context: {{context}}

Please provide:
1. Main topics/entities mentioned
2. Intent of the query
3. Time sensitivity (if any)
4. Domain specificity

Respond in JSON format."#,
        )
        .unwrap()
        .with_description("Analyzes user queries to extract structured information");

        self.register(query_analysis);

        // Conflict resolution template
        let conflict_resolution = PromptTemplate::new(
            "conflict_resolution",
            r#"You are analyzing conflicting pieces of knowledge:

Claim A: {{claim_a}}
Source A: {{source_a}}

Claim B: {{claim_b}}
Source B: {{source_b}}

Analyze the conflict and provide:
1. Type of conflict (factual, temporal, perspective-based)
2. Possible resolution or synthesis
3. Confidence in your analysis

Respond in JSON format."#,
        )
        .unwrap()
        .with_description("Analyzes conflicts between knowledge claims");

        self.register(conflict_resolution);

        // Belief synthesis template
        let belief_synthesis = PromptTemplate::new(
            "belief_synthesis",
            r#"Synthesize the following related claims into a coherent belief:

{{claims}}

Provide:
1. A unified statement that best represents the claims
2. Key supporting points
3. Any uncertainties or caveats
4. Confidence level (0.0-1.0)"#,
        )
        .unwrap()
        .with_description("Synthesizes multiple claims into a consolidated belief");

        self.register(belief_synthesis);

        // Prediction generation template
        let prediction = PromptTemplate::new(
            "prediction",
            r#"Based on the following knowledge context, make a prediction:

Context:
{{context}}

Question: {{question}}

Provide:
1. Your prediction
2. Reasoning based on the context
3. Confidence level (0.0-1.0)
4. Key factors that could change the outcome"#,
        )
        .unwrap()
        .with_description("Generates predictions based on knowledge context");

        self.register(prediction);

        // Knowledge gap analysis template
        let gap_analysis = PromptTemplate::new(
            "gap_analysis",
            r#"Analyze the following query and knowledge context to identify knowledge gaps:

Query: {{query}}
Available Knowledge: {{knowledge}}

Identify:
1. What information is missing to fully answer the query
2. What sources might fill these gaps
3. Priority of each gap (high/medium/low)"#,
        )
        .unwrap()
        .with_description("Identifies knowledge gaps in query responses");

        self.register(gap_analysis);

        // Document classification template
        let doc_classification = PromptTemplate::new(
            "doc_classification",
            r#"Classify the following document:

Filename: {{filename}}
Content Preview: {{preview}}

Extract:
1. Document type (report, methodology, case study, etc.)
2. Industry/domain (if identifiable)
3. Key entities mentioned (companies, products, people)
4. Key topics/themes
5. Any predictions or claims made

Respond in JSON format."#,
        )
        .unwrap()
        .with_description("Classifies documents and extracts metadata");

        self.register(doc_classification);
    }
}

pub fn create_system_prompt(purpose: &str) -> String {
    match purpose {
        "query_analysis" => r#"You are a query analysis assistant. Your role is to understand user queries deeply and extract structured information that will help retrieve relevant knowledge. Be thorough but concise."#.to_string(),

        "conflict_resolution" => r#"You are a knowledge conflict analyzer. Your role is to identify the nature of conflicts between knowledge claims and suggest resolutions. Be objective and consider multiple perspectives."#.to_string(),

        "belief_synthesis" => r#"You are a knowledge synthesis assistant. Your role is to combine multiple related claims into coherent beliefs, maintaining appropriate confidence levels based on the quality and consistency of sources."#.to_string(),

        "prediction" => r#"You are a prediction assistant working with an epistemic knowledge system. Your role is to make informed predictions based on available knowledge, clearly indicating confidence levels and key factors."#.to_string(),

        "knowledge_extraction" => r#"You are a knowledge extraction assistant. Your role is to extract structured claims, entities, and relationships from documents and text. Be precise and focus on factual content."#.to_string(),

        "entity_extraction" => r#"You are an entity extraction specialist. Extract named entities (companies, people, products, locations) from the provided text, along with their types and any relevant attributes."#.to_string(),

        _ => r#"You are a helpful AI assistant integrated with an epistemic knowledge system. Provide accurate, well-reasoned responses based on available information."#.to_string(),
    }
}

pub fn build_messages(system: Option<String>, user_prompt: String) -> Vec<Message> {
    let mut messages = Vec::new();

    if let Some(system_content) = system {
        messages.push(Message {
            role: crate::types::Role::System,
            content: system_content,
        });
    }

    messages.push(Message {
        role: crate::types::Role::User,
        content: user_prompt,
    });

    messages
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_new_extracts_variables() {
        let t = PromptTemplate::new("test", "Hello {{var1}} and {{var2}}").unwrap();
        assert_eq!(t.variables, vec!["var1".to_string(), "var2".to_string()]);
    }

    #[test]
    fn template_new_no_variables() {
        let t = PromptTemplate::new("test", "No variables here").unwrap();
        assert!(t.variables.is_empty());
    }

    #[test]
    fn template_render_substitutes() {
        let t = PromptTemplate::new("test", "Hello {{name}}").unwrap();
        let mut vars = HashMap::new();
        vars.insert("name".to_string(), "World".to_string());
        let result = t.render(&vars).unwrap();
        assert_eq!(result, "Hello World");
    }

    #[test]
    fn template_render_missing_variable_error() {
        let t = PromptTemplate::new("test", "Hello {{name}}").unwrap();
        let vars = HashMap::new();
        assert!(t.render(&vars).is_err());
    }

    #[test]
    fn template_render_uses_defaults() {
        let mut defaults = HashMap::new();
        defaults.insert("name".to_string(), "Default".to_string());
        let t = PromptTemplate::new("test", "Hello {{name}}")
            .unwrap()
            .with_defaults(defaults);
        let vars = HashMap::new();
        let result = t.render(&vars).unwrap();
        assert_eq!(result, "Hello Default");
    }

    #[test]
    fn template_with_description() {
        let t = PromptTemplate::new("test", "template")
            .unwrap()
            .with_description("A description");
        assert_eq!(t.description, "A description");
    }

    #[test]
    fn template_extract_variables_deduplicates() {
        let t = PromptTemplate::new("test", "{{var}} and {{var}} again").unwrap();
        assert_eq!(t.variables, vec!["var".to_string()]);
    }

    #[test]
    fn manager_new_has_defaults() {
        let mgr = TemplateManager::new();
        assert!(!mgr.list().is_empty());
    }

    #[test]
    fn manager_register_and_get() {
        let mut mgr = TemplateManager::new();
        let t = PromptTemplate::new("custom", "Hello {{x}}").unwrap();
        mgr.register(t);
        assert!(mgr.get("custom").is_some());
    }

    #[test]
    fn manager_remove() {
        let mut mgr = TemplateManager::new();
        let t = PromptTemplate::new("removable", "text").unwrap();
        mgr.register(t);
        let removed = mgr.remove("removable");
        assert!(removed.is_some());
        assert!(mgr.get("removable").is_none());
    }

    #[test]
    fn manager_list() {
        let mgr = TemplateManager::new();
        let templates = mgr.list();
        assert!(templates.len() >= 1);
    }

    #[test]
    fn manager_render() {
        let mgr = TemplateManager::new();
        let mut vars = HashMap::new();
        vars.insert("query".to_string(), "test query".to_string());
        vars.insert("context".to_string(), "test context".to_string());
        let result = mgr.render("query_analysis", &vars);
        assert!(result.is_ok());
    }

    #[test]
    fn manager_render_missing_template_error() {
        let mgr = TemplateManager::new();
        let vars = HashMap::new();
        assert!(mgr.render("nonexistent", &vars).is_err());
    }

    #[test]
    fn create_system_prompt_query_analysis() {
        let prompt = create_system_prompt("query_analysis");
        assert!(!prompt.is_empty());
    }

    #[test]
    fn create_system_prompt_prediction() {
        let prompt = create_system_prompt("prediction");
        assert!(!prompt.is_empty());
    }

    #[test]
    fn create_system_prompt_unknown() {
        let prompt = create_system_prompt("unknown_purpose");
        assert!(!prompt.is_empty());
    }

    #[test]
    fn build_messages_with_system() {
        let msgs = build_messages(Some("system msg".to_string()), "user msg".to_string());
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, crate::types::Role::System);
        assert_eq!(msgs[1].role, crate::types::Role::User);
    }

    #[test]
    fn build_messages_without_system() {
        let msgs = build_messages(None, "user msg".to_string());
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, crate::types::Role::User);
    }
}
