use crate::config_manager::SafeSend;
use crate::socket_utils::{ClientMsg, DownloadState, Filter, ServerMsg};
use crate::tui::app::{App, ListState};
use crate::tui::confirm_widget::ActionConfirm;
use crate::tui::loading_widget::LoadingState;
use crate::tui::notification_widget::Notification;
use crate::tui::progress_bar::{BasicBar, SimpleBar};
use crate::tui::ui::{self, CurrentScreen, InputState, Popup};
use crate::{END_NOTIFY, READY_TO_EXIT};
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use std::io;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedReceiver;

macro_rules! check_login {
    ($app:expr) => {
        if !$app.is_logged_in {
            let noti = Notification::new("Failed".to_string(), "Please login first!".to_string())
                .duration(Duration::from_secs(2));
            $app.notifications_queue.push_back(noti);
            return false;
        }
    };
}

pub enum LEvent {
    Tui(Event),
    Render,
    Socket(ServerMsg),
}

impl LEvent {
    pub async fn event_loop(app: &mut App, mut rx: UnboundedReceiver<LEvent>) -> io::Result<()> {
        while let Some(event) = rx.recv().await {
            match event {
                LEvent::Tui(ui_event) => {
                    // deal with keyboard...
                    if let Event::Key(key) = ui_event
                        && Self::handle_key_events(app, key)
                    {
                        break;
                    }
                }
                LEvent::Render => {
                    // render ui here
                    ui::render(app)?;
                }
                LEvent::Socket(msg) => {
                    // deal with socket message here
                    match msg {
                        ServerMsg::Download(msg) => match msg.state {
                            DownloadState::Start(ptr) => {
                                let (name, size) = *ptr;
                                log::trace!(
                                    "received a socket download start msg, name: {}, {}, size: {}",
                                    name,
                                    msg.id,
                                    size
                                );
                                let bar = SimpleBar::new(name, size);
                                app.downloading_state.progress_suit.add(msg.id, bar);
                            }
                            DownloadState::Downloading(_) => (),
                            DownloadState::Finished => {
                                log::trace!("received a socket download finish msg, {}", msg.id);
                                if let Some(bar) =
                                    app.downloading_state.progress_suit.get_bar_mut(msg.id)
                                {
                                    bar.inc_to_finished();
                                } else {
                                    log::error!(
                                        "received a finished msg, but can not find its progress bar"
                                    );
                                    app.socket_tx.send_msg(ClientMsg::SyncQuery);
                                }
                            }
                            DownloadState::Failed => {
                                log::trace!("received a socket download failed msg, {}", msg.id);
                                app.downloading_state.progress_suit.remove(msg.id);
                            }
                        },
                        ServerMsg::DownloadSync(state) => {
                            // log::trace!("received a socket download sync msg");
                            let mut is_lossy = false;
                            for s in &state {
                                if let Some(bar) =
                                    app.downloading_state.progress_suit.get_bar_mut(s.id)
                                {
                                    bar.set_current_size(s.current_size);
                                    bar.set_current_speed(s.current_speed);
                                } else {
                                    log::error!(
                                        "received a sync msg, but can not find its progress bar"
                                    );
                                    is_lossy = true;
                                }
                            }
                            // maybe we should not be too strict
                            // if is_lossy || state.len() != app.downloading_state.progress_suit.len()
                            // {
                            if is_lossy {
                                log::error!("send SyncQuery because of DownloadSync");
                                app.socket_tx.send_msg(ClientMsg::SyncQuery);
                            }
                        }
                        ServerMsg::SyncResp(mut info) => {
                            app.downloading_state.progress_suit = info.progresses;
                            // sort by last_update in descending order
                            info.animes
                                .sort_by(|a, b| b.last_update.cmp(&a.last_update));
                            if let Some(index) = app.rss_state.selected() {
                                let current_id = &app.rss_data[index].id;
                                app.rss_state
                                    .select(info.animes.iter().enumerate().find_map(
                                        |(i, anime)| {
                                            if &anime.id == current_id {
                                                Some(i)
                                            } else {
                                                None
                                            }
                                        },
                                    ));
                            }
                            app.rss_data = info.animes;
                            // log::info!("{:#?}", info.animes);
                        }
                        ServerMsg::RSSData(mut animes) => {
                            // clear loading animation
                            app.loading_state = None;
                            // sort by last_update in descending order
                            animes.sort_by(|a, b| b.last_update.cmp(&a.last_update));
                            if let Some(index) = app.rss_state.selected() {
                                let current_id = &app.rss_data[index].id;
                                app.rss_state.select(animes.iter().enumerate().find_map(
                                    |(i, anime)| {
                                        if &anime.id == current_id {
                                            Some(i)
                                        } else {
                                            None
                                        }
                                    },
                                ));
                            }
                            app.rss_data = animes.into_vec();
                            log::info!("successfully updated RSS");
                        }
                        ServerMsg::Ok(info) => {
                            log::info!("{}", info);
                            let noti = Notification::new("Success".to_string(), info.into_string());
                            app.notifications_queue.push_back(noti);
                        }
                        ServerMsg::Error(ptr) => {
                            let (info, error) = *ptr;
                            log::error!("{}", error);
                            let noti = Notification::new("Failed".to_string(), info);
                            app.notifications_queue.push_back(noti);
                        }
                        ServerMsg::Info(info) => {
                            log::info!("{}", info);
                            let noti = Notification::new("Info".to_string(), info.into_string());
                            app.notifications_queue.push_back(noti);
                        }
                        ServerMsg::LoginState(state) => {
                            log::info!("Login state: {}", state);
                            let noti =
                                Notification::new("Login State".to_string(), state.into_string())
                                    .duration(Duration::from_secs(2));
                            app.notifications_queue.push_back(noti);
                        }
                        ServerMsg::IsLogin(login) => {
                            app.is_logged_in = login;
                        }
                        ServerMsg::QrcodeExpired => {
                            app.qrcode_url = Err("Qrcode expired, please reopen the login popup");
                        }
                        ServerMsg::LoginUrl(url) => {
                            log::info!("Login URL: {}", url);
                            app.qrcode_url = Ok(url);
                        }
                        ServerMsg::Loading => {
                            app.loading_state = Some(LoadingState::new());
                        }
                        ServerMsg::SubFilter(filters) => {
                            app.filter_id_state.select(None);
                            app.filter_rule_state.select(None);
                            app.filters = filters.into_vec();
                        }
                        ServerMsg::WaitingState(state) => {
                            app.waiting_state = state;
                        }
                        ServerMsg::Exit => {
                            log::info!("Received exit message, exiting...");
                            READY_TO_EXIT.store(true, std::sync::atomic::Ordering::Relaxed);
                            END_NOTIFY.notify_waiters();
                            return Ok(());
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// bool: whether to exit
    pub fn handle_key_events(app: &mut App, key: KeyEvent) -> bool {
        // IMPORTANT: return early if there is any modifier
        if !key.modifiers.is_empty() {
            match key.modifiers {
                KeyModifiers::CONTROL => {
                    if let KeyCode::Char('a') = key.code
                        && let InputState::Text(editor) = &mut app.input_state
                    {
                        log::info!("selected!");
                        editor.select_all();
                    }
                }
                KeyModifiers::SHIFT => {
                    if let InputState::Text(editor) = &mut app.input_state {
                        match key.code {
                            KeyCode::Left => {
                                log::info!("shift with left arrow");
                                editor.left_arrow_shift();
                            }
                            KeyCode::Right => {
                                log::info!("shift with right arrow");
                                editor.right_arrow_shift();
                            }
                            _ => (),
                        }
                    }
                }
                KeyModifiers::ALT if app.current_screen == CurrentScreen::Filter => {
                    match key.code {
                        KeyCode::Up => {
                            if let Some(index) = app.filter_rule_state.selected()
                                && index > 0
                            {
                                let filter =
                                    &mut app.filters[app.filter_id_state.selected().unwrap()];
                                filter.subgroup.filter_list.swap(index, index - 1);
                                app.filter_rule_state.select(Some(index - 1));
                                app.socket_tx
                                    .send_msg(ClientMsg::InsertFilter(filter.clone()));
                            }
                        }
                        KeyCode::Down => {
                            if let Some(index) = app.filter_rule_state.selected() {
                                let filter =
                                    &mut app.filters[app.filter_id_state.selected().unwrap()];
                                let filter_list = &mut filter.subgroup.filter_list;
                                if index + 1 < filter_list.len() {
                                    filter_list.swap(index, index + 1);
                                    app.filter_rule_state.select(Some(index + 1));
                                    app.socket_tx
                                        .send_msg(ClientMsg::InsertFilter(filter.clone()));
                                }
                            }
                        }
                        _ => (),
                    }
                }
                _ => (),
            }
            return false;
        }

        match key.code {
            KeyCode::Char(char) => {
                match &mut app.input_state {
                    InputState::NotInput => match char {
                        // press 'q' to exit
                        'q' => {
                            if app.current_popup.is_none() {
                                READY_TO_EXIT.store(true, std::sync::atomic::Ordering::Relaxed);
                                app.socket_tx.send_msg(ClientMsg::Exit);
                                // wait for the exit message to be handled
                                std::thread::sleep(Duration::from_millis(50));
                                END_NOTIFY.notify_waiters();
                                return true;
                            }
                        }
                        // press '1' to switch to main screen
                        '1' => {
                            app.current_screen = CurrentScreen::Main;
                        }
                        '2' => {
                            app.current_screen = CurrentScreen::Downloading;
                        }
                        '3' => {
                            app.current_screen = CurrentScreen::Finished;
                        }
                        '4' => {
                            if app.current_screen != CurrentScreen::Filter {
                                app.current_screen = CurrentScreen::Filter;
                                app.filters.clear();
                                app.socket_tx.send_msg(ClientMsg::GetFilters);
                            }
                        }
                        '5' => {
                            app.current_screen = CurrentScreen::State;
                        }
                        '6' => {
                            app.current_screen = CurrentScreen::Log;
                        }
                        char if app.current_screen == CurrentScreen::Main
                            && app.current_popup.is_none() =>
                        {
                            match char {
                                // refresh RSS
                                'r' => {
                                    check_login!(app);
                                    app.socket_tx.send_msg(ClientMsg::RefreshRSS);
                                    let loading_state = LoadingState::new();
                                    app.loading_state = Some(loading_state);
                                }
                                // download a folder
                                'd' => {
                                    check_login!(app);
                                    app.input_state = InputState::empty_text();
                                    app.current_popup = Some(Popup::DownloadFolder);
                                }
                                // login to cloud
                                'l' => {
                                    app.current_popup = Some(Popup::Login);
                                    app.socket_tx.send_msg(ClientMsg::LoginReq);
                                }
                                // add rss link
                                'a' => {
                                    // check_login!(app);
                                    app.input_state = InputState::empty_text();
                                    app.current_popup = Some(Popup::AddRSSLink);
                                }
                                // delete rss link
                                'D' => {
                                    if let Some(index) = app.rss_state.selected() {
                                        let anime = &app.rss_data[index];
                                        let anime_id = anime.id.clone();
                                        let action = Box::new(move |app: &mut App| {
                                            let index = app
                                                .rss_data
                                                .iter()
                                                .position(|anime| anime.id == anime_id);
                                            if let Some(index) = index {
                                                app.rss_data.remove(index);
                                                app.socket_tx.send_msg(ClientMsg::DeleteAnime(
                                                    anime_id.into_boxed_str(),
                                                ));
                                            } else {
                                                log::error!("bangumi rss link is already removed")
                                            }
                                        });
                                        let question = "Do you want to delete this bangumi?";
                                        let content = anime.name.clone();
                                        let action_confirm = ActionConfirm::new(
                                            question.into(),
                                            content.into(),
                                            action,
                                        );
                                        app.current_popup = Some(Popup::Confirm(action_confirm));
                                    }
                                }
                                _ => (),
                            }
                        }
                        char if app.current_popup.is_some() => {
                            let popup = unsafe { app.current_popup.as_ref().unwrap_unchecked() };
                            if matches!(popup, Popup::Confirm(_)) {
                                match char {
                                    'y' | 'Y' => {
                                        let confirm = match app.current_popup.take() {
                                            Some(Popup::Confirm(confirm)) => confirm,
                                            _ => unsafe { core::hint::unreachable_unchecked() },
                                        };
                                        (confirm.action)(app);
                                    }
                                    'n' | 'N' => {
                                        app.current_popup = None;
                                    }
                                    _ => (),
                                }
                            }
                        }
                        char if app.current_screen == CurrentScreen::Filter => match char {
                            // add a subgroup or a rule after current
                            'a' => match app.filter_rule_state.selected() {
                                // we have ensured that `index` is always in bound
                                Some(index) => {
                                    let rules = &mut app.filters
                                        [app.filter_id_state.selected().unwrap()]
                                    .subgroup
                                    .filter_list;
                                    rules.insert(index + 1, String::new());
                                    app.filter_rule_state.select(Some(index + 1));
                                    app.input_state = InputState::empty_text();
                                }
                                None => {
                                    if let Some(index) = app.filter_id_state.selected() {
                                        let filter = Filter::default();
                                        app.filters.insert(index + 1, filter);
                                        app.filter_id_state.select(Some(index + 1));
                                        app.input_state = InputState::empty_text();
                                    }
                                }
                            },
                            // add a subgroup or a rule before current
                            'i' => match app.filter_rule_state.selected() {
                                Some(index) => {
                                    let rules = &mut app.filters
                                        [app.filter_id_state.selected().unwrap()]
                                    .subgroup
                                    .filter_list;
                                    rules.insert(index, String::new());
                                    app.input_state = InputState::empty_text();
                                }
                                None => {
                                    if let Some(index) = app.filter_id_state.selected() {
                                        let filter = Filter::default();
                                        app.filters.insert(index, filter);
                                        app.input_state = InputState::empty_text();
                                    }
                                }
                            },
                            // edit a subgroup or a rule
                            'e' => match app.filter_rule_state.selected() {
                                Some(index) => {
                                    let rule = &app.filters
                                        [app.filter_id_state.selected().unwrap()]
                                    .subgroup
                                    .filter_list[index];
                                    app.input_state = InputState::text(rule.clone());
                                }
                                None => {
                                    if let Some(index) = app.filter_id_state.selected() {
                                        app.input_state =
                                            InputState::text(app.filters[index].id.clone());
                                    }
                                }
                            },
                            // delete a subgroup or a rule
                            'D' => match app.filter_rule_state.selected() {
                                Some(index) => {
                                    let filter =
                                        &mut app.filters[app.filter_id_state.selected().unwrap()];
                                    let id = filter.id.clone();
                                    let rule_name = filter.subgroup.filter_list[index].clone();
                                    let content = rule_name.clone();
                                    let action = Box::new(move |app: &mut App| {
                                        let filter = app.filters.iter_mut().find(|f| f.id == id);
                                        if let Some(filter) = filter
                                            && let Some(index) = filter
                                                .subgroup
                                                .filter_list
                                                .iter()
                                                .position(|r| *r == rule_name)
                                        {
                                            filter.subgroup.filter_list.remove(index);
                                            app.socket_tx
                                                .send_msg(ClientMsg::InsertFilter(filter.clone()));
                                        } else {
                                            log::error!("filter rule is already removed");
                                        }
                                    });
                                    let question = "Do you want to delete this rule?";
                                    let action_confirm =
                                        ActionConfirm::new(question.into(), content.into(), action);
                                    app.current_popup = Some(Popup::Confirm(action_confirm));
                                }
                                None => {
                                    if let Some(index) = app.filter_id_state.selected() {
                                        let filter = &mut app.filters[index];
                                        let filter_id = filter.id.clone();
                                        let content =
                                            format!("{} {}", filter.id, filter.subgroup.name);
                                        let action = Box::new(move |app: &mut App| {
                                            let index =
                                                app.filters.iter().position(|f| f.id == filter_id);
                                            if let Some(index) = index {
                                                app.filters.remove(index);
                                                app.socket_tx
                                                    .send_msg(ClientMsg::DelFilter(filter_id));
                                            } else {
                                                log::error!("the filter is already removed");
                                            }
                                        });
                                        let question = "Do you want to delete this filter?";
                                        let action_confirm = ActionConfirm::new(
                                            question.into(),
                                            content.into(),
                                            action,
                                        );
                                        app.current_popup = Some(Popup::Confirm(action_confirm));
                                    }
                                }
                            },
                            _ => (),
                        },
                        char if app.current_screen == CurrentScreen::State => match char {
                            'r' if app.waiting_state.waiting_count > 0 => {
                                // send recovery signal
                                app.socket_tx.send_msg(ClientMsg::Recover);
                            }
                            _ => (),
                        },
                        _ => (),
                    },
                    InputState::Text(str) => {
                        str.insert(char);
                    }
                }
            }
            KeyCode::Esc => {
                // assume the popup takes the keyboard focus and we don't want to restore it
                if app.current_popup.is_some() {
                    app.current_popup = None;
                    app.input_state = InputState::NotInput;
                } else if app.current_screen == CurrentScreen::Filter && app.input_state.is_typing()
                {
                    app.input_state = InputState::NotInput;
                    match app.filter_rule_state.selected() {
                        Some(index) => {
                            let filter_list = &mut app.filters
                                [app.filter_id_state.selected().unwrap()]
                            .subgroup
                            .filter_list;
                            if filter_list[index].is_empty() {
                                filter_list.remove(index);
                            }
                        }
                        None => {
                            if let Some(index) = app.filter_id_state.selected()
                                && app.filters[index].id.is_empty()
                            {
                                app.filters.remove(index);
                            }
                        }
                    }
                } else if app.current_screen != CurrentScreen::Main {
                    app.current_screen = CurrentScreen::Main;
                }
            }
            KeyCode::Backspace => {
                if let InputState::Text(editor) = &mut app.input_state {
                    editor.backspace();
                }
            }
            KeyCode::Enter => {
                if let Some(popup) = &app.current_popup {
                    match popup {
                        Popup::DownloadFolder => {
                            if let InputState::Text(editor) = app.input_state.take()
                                && !editor.is_empty()
                            {
                                let msg = ClientMsg::DownloadFolder(
                                    editor.into_string().into_boxed_str(),
                                );
                                app.socket_tx.send_msg(msg);
                                app.input_state = InputState::NotInput;
                                app.current_popup = None;
                            }
                        }
                        Popup::AddRSSLink => {
                            if let InputState::Text(editor) = app.input_state.take()
                                && !editor.is_empty()
                            {
                                let msg = ClientMsg::AddRSS(editor.into_string().into_boxed_str());
                                app.socket_tx.send_msg(msg);
                                app.input_state = InputState::NotInput;
                                app.current_popup = None;
                            }
                        }
                        _ => (),
                    }
                } else if let CurrentScreen::Filter = app.current_screen
                    && app.input_state.is_typing()
                {
                    match app.filter_rule_state.selected() {
                        Some(index) => {
                            let filter = &mut app.filters[app.filter_id_state.selected().unwrap()];
                            let rules = &mut filter.subgroup.filter_list;
                            let old_rule = &rules[index];
                            if let InputState::Text(editor) = app.input_state.take()
                                && !editor.is_empty()
                            {
                                let editor_str = editor.into_string();
                                if !rules.contains(&editor_str) {
                                    rules[index] = editor_str;
                                    app.socket_tx
                                        .send_msg(ClientMsg::InsertFilter(filter.clone()));
                                    app.input_state = InputState::NotInput;
                                } else {
                                    if old_rule != &editor_str {
                                        let noti = Notification::new(
                                            "Failed".to_string(),
                                            "This rule already exists!".to_string(),
                                        );
                                        app.notifications_queue.push_back(noti);
                                    }
                                    app.input_state = InputState::text(editor_str);
                                }
                            }
                        }
                        None => {
                            if let Some(index) = app.filter_id_state.selected() {
                                let old_filter_id = &app.filters[index].id;
                                if let InputState::Text(editor) = app.input_state.take()
                                    && !editor.is_empty()
                                {
                                    let editor_str = editor.into_string();
                                    if app.filters.iter().any(|f| f.id == editor_str) {
                                        // if old id is not empty, it means that we are editing the old id,
                                        // so we should delete the old one first
                                        if !old_filter_id.is_empty() {
                                            app.socket_tx.send_msg(ClientMsg::DelFilter(
                                                old_filter_id.clone(),
                                            ));
                                        }
                                        let filter = &mut app.filters[index];
                                        filter.id = editor_str;
                                        app.socket_tx
                                            .send_msg(ClientMsg::InsertFilter(filter.clone()));
                                        app.input_state = InputState::NotInput;
                                    } else {
                                        // the new id is already in the list, but it may be equal to the
                                        // old one, at this situation we should do nothing. If not, it means
                                        // the id is duplicate, so send a nofication here.
                                        if old_filter_id != &editor_str {
                                            let noti = Notification::new(
                                                "Failed".to_string(),
                                                "This filter already exists!".to_string(),
                                            );
                                            app.notifications_queue.push_back(noti);
                                        }
                                        app.input_state = InputState::text(editor_str);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            KeyCode::Down => {
                let scroll_down = |state: &mut ListState| {
                    if state.offset + 1 < state.progress_suit.len() {
                        state.offset += 1;
                        state.scroll_state = state.scroll_state.position(state.offset);
                    }
                };
                match &app.current_screen {
                    CurrentScreen::Main
                        if app.current_popup.is_none() && !app.rss_data.is_empty() =>
                    {
                        match app.rss_state.selected() {
                            Some(index) => {
                                if index + 1 < app.rss_data.len() {
                                    app.rss_state.select(Some(index + 1));
                                }
                            }
                            None => {
                                app.rss_state.select(Some(0));
                            }
                        }
                    }
                    CurrentScreen::Downloading => {
                        let state = &mut app.downloading_state;
                        scroll_down(state);
                    }
                    CurrentScreen::Finished => {
                        let state = &mut app.finished_state;
                        scroll_down(state);
                    }
                    CurrentScreen::Filter
                        if !app.filters.is_empty() && !app.input_state.is_typing() =>
                    {
                        match app.filter_rule_state.selected() {
                            Some(index) => {
                                let list_len = app.filters[app.filter_id_state.selected().unwrap()]
                                    .subgroup
                                    .filter_list
                                    .len();
                                if index + 1 < list_len {
                                    app.filter_rule_state.select(Some(index + 1));
                                }
                            }
                            None => match app.filter_id_state.selected() {
                                Some(index) => {
                                    if index + 1 < app.filters.len() {
                                        app.filter_id_state.select(Some(index + 1));
                                    }
                                }
                                None => {
                                    app.filter_id_state.select(Some(0));
                                }
                            },
                        }
                    }
                    CurrentScreen::Log => {
                        app.log_widget_state
                            .transition(tui_logger::TuiWidgetEvent::NextPageKey);
                    }
                    _ => (),
                }
            }
            KeyCode::Up => {
                let scroll_up = |state: &mut ListState| {
                    if state.offset > 0 {
                        state.offset -= 1;
                        state.scroll_state = state.scroll_state.position(state.offset);
                    }
                };
                match &mut app.current_screen {
                    CurrentScreen::Main
                        if app.current_popup.is_none() && !app.rss_data.is_empty() =>
                    {
                        match app.rss_state.selected() {
                            Some(index) => {
                                app.rss_state.select(Some(index.saturating_sub(1)));
                            }
                            None => {
                                app.rss_state.select(Some(app.rss_data.len() - 1));
                            }
                        }
                    }
                    CurrentScreen::Downloading => {
                        let state = &mut app.downloading_state;
                        scroll_up(state);
                    }
                    CurrentScreen::Finished => {
                        let state = &mut app.finished_state;
                        scroll_up(state);
                    }
                    CurrentScreen::Filter
                        if !app.filters.is_empty() && !app.input_state.is_typing() =>
                    {
                        match app.filter_rule_state.selected() {
                            Some(index) => {
                                app.filter_rule_state.select(Some(index.saturating_sub(1)));
                            }
                            None => match app.filter_id_state.selected() {
                                Some(index) => {
                                    app.filter_id_state.select(Some(index.saturating_sub(1)));
                                }
                                None => {
                                    app.filter_id_state.select(Some(app.filters.len() - 1));
                                }
                            },
                        }
                    }
                    CurrentScreen::Log => {
                        app.log_widget_state
                            .transition(tui_logger::TuiWidgetEvent::PrevPageKey);
                    }
                    _ => (),
                }
            }
            KeyCode::Right => {
                if let InputState::Text(editor) = &mut app.input_state {
                    editor.right_arrow();
                } else if let CurrentScreen::Filter = app.current_screen
                    && app.filter_rule_state.selected().is_none()
                    && let Some(index) = app.filter_id_state.selected()
                {
                    if !app.filters[index].subgroup.filter_list.is_empty() {
                        app.filter_rule_state.select(Some(0));
                    } else {
                        app.filters[index].subgroup.filter_list.push(String::new());
                        app.input_state = InputState::empty_text();
                        app.filter_rule_state.select(Some(0));
                    }
                }
            }
            KeyCode::Left => {
                if let InputState::Text(editor) = &mut app.input_state {
                    editor.left_arrow();
                } else if let CurrentScreen::Filter = app.current_screen
                    && app.filter_rule_state.selected().is_some()
                {
                    app.filter_rule_state.select(None);
                }
            }
            _ => {}
        }
        false
    }
}
