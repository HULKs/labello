impl LabelloApp {
    pub(crate) fn visible_prelabels(&self) -> Vec<labello_domain::PrelabelSuggestion> {
        if self.view != AppView::Annotate { return Vec::new(); }
        let Some(current) = &self.work.current else { return Vec::new(); };
        let Some(task) = self.selected_task() else { return Vec::new(); };
        let committed: std::collections::BTreeSet<_> = self.work.current_state.iter()
            .flat_map(|state| state.annotations.values()).filter_map(|versions| versions.last())
            .filter_map(|annotation| match &annotation.origin {
                labello_domain::AnnotationOrigin::Prelabel { prelabel } => Some(prelabel.provenance.suggestion_id.as_str()),
                _ => None,
            }).collect();
        let mut visible = current.prelabels.iter().filter(|hint| hint.task_id == task.task_id
            && self.selected_class_id() == Some(&hint.class_id)
            && !self.work.accepted_prelabels.contains(&hint.suggestion_id)
            && !committed.contains(hint.suggestion_id.as_str())).cloned().collect::<Vec<_>>();
        let mut objects = self.work.prelabel_review.objects.iter()
            .filter(|item| visible.iter().any(|hint| hint.suggestion_id == item.suggestion.suggestion_id))
            .map(|item| item.annotation.clone()).collect();
        self.filter_visible_boxes(&mut objects);
        visible.retain(|hint| self.work.prelabel_review.objects.iter()
            .find(|item| item.suggestion.suggestion_id == hint.suggestion_id)
            .is_none_or(|item| objects.iter().any(|annotation| annotation.annotation_id == item.annotation.annotation_id)));
        visible
    }
}
