use ai::LLMId;
use warp_core::telemetry::testing::MockTelemetryContextProvider;
use warpui_core::{App, Entity, ModelHandle};

use crate::model::{
    AiSetupChoice, CreditPackOption, CreditPurchaseState, OnboardingAuthState,
    OnboardingStateEvent, OnboardingStateModel, OnboardingStep,
};
use crate::slides::OfferVariant;

fn add_test_model(app: &mut App) -> ModelHandle<OnboardingStateModel> {
    app.update(MockTelemetryContextProvider::register);
    add_model(app)
}

#[test]
fn pricing_promotion_message_can_be_replaced_and_cleared() {
    App::test((), |mut app| async move {
        let model = add_test_model(&mut app);
        model.read(&app, |model, _| {
            assert_eq!(model.pricing_promotion_message(), None);
        });

        model.update(&mut app, |model, ctx| {
            model.set_pricing_promotion_message(Some("50% off Fable and Opus 5".to_string()), ctx)
        });
        model.read(&app, |model, _| {
            assert_eq!(
                model.pricing_promotion_message(),
                Some("50% off Fable and Opus 5")
            );
        });

        model.update(&mut app, |model, ctx| {
            model.set_pricing_promotion_message(None, ctx)
        });
        model.read(&app, |model, _| {
            assert_eq!(model.pricing_promotion_message(), None);
        });
    });
}

fn add_model(app: &mut App) -> ModelHandle<OnboardingStateModel> {
    app.add_model(|_| {
        OnboardingStateModel::new(
            Vec::new(),
            LLMId::from("auto"),
            false,
            true,
            OnboardingAuthState::FreeUser,
        )
    })
}

fn step(app: &App, model: &ModelHandle<OnboardingStateModel>) -> OnboardingStep {
    model.read(app, |model, _| model.step())
}

#[test]
fn post_auth_offer_is_unclassified_until_selected_and_does_not_switch() {
    App::test((), |mut app| async move {
        let model = add_test_model(&mut app);
        model.read(&app, |model, _| {
            assert_eq!(model.offer_variant(), None);
        });
        model.update(&mut app, |model, ctx| {
            model.show_post_auth_offer(OfferVariant::HeadStart, ctx);
            model.show_post_auth_offer(OfferVariant::ChooseHowToStart, ctx);
        });

        assert_eq!(step(&app, &model), OnboardingStep::PostAuthOffer);
        model.read(&app, |model, _| {
            assert_eq!(model.offer_variant(), Some(OfferVariant::HeadStart));
        });
    });
}


fn credit_packs() -> Vec<CreditPackOption> {
    vec![
        CreditPackOption {
            credits: 400,
            price_usd_cents: 1_200,
            savings_percent: 0,
        },
        CreditPackOption {
            credits: 1_000,
            price_usd_cents: 2_400,
            savings_percent: 20,
        },
    ]
}

fn purchase_state(app: &App, model: &ModelHandle<OnboardingStateModel>) -> CreditPurchaseState {
    model.read(app, |model, _| model.credit_purchase_state())
}

/// A do-nothing model used only to count the completion events the onboarding
/// model emits. Completion is an event rather than a state change, so it can't
/// be read back off the model itself.
#[derive(Default)]
struct CompletionObserver {
    completions: usize,
}

impl Entity for CompletionObserver {
    type Event = ();
}

fn observe_completions(
    app: &mut App,
    model: &ModelHandle<OnboardingStateModel>,
) -> ModelHandle<CompletionObserver> {
    let model = model.clone();
    app.add_model(move |ctx| {
        ctx.subscribe_to_model(&model, |observer: &mut CompletionObserver, _, event, _| {
            if matches!(event, OnboardingStateEvent::CreditPurchaseCompleted) {
                observer.completions += 1;
            }
        });
        CompletionObserver::default()
    })
}

fn completions(app: &App, observer: &ModelHandle<CompletionObserver>) -> usize {
    observer.read(app, |observer, _| observer.completions)
}

#[test]
fn credit_packs_default_to_the_first_option_and_are_selectable() {
    App::test((), |mut app| async move {
        let model = add_test_model(&mut app);
        model.read(&app, |model, _| {
            assert!(model.credit_pack_options().is_empty());
            assert_eq!(model.selected_credit_pack(), None);
        });

        model.update(&mut app, |model, ctx| {
            model.set_credit_pack_options(credit_packs(), ctx)
        });
        model.read(&app, |model, _| {
            assert_eq!(model.selected_credit_pack_index(), 0);
            assert_eq!(model.selected_credit_pack().map(|p| p.credits), Some(400));
        });

        model.update(&mut app, |model, ctx| model.select_credit_pack(1, ctx));
        model.read(&app, |model, _| {
            assert_eq!(model.selected_credit_pack().map(|p| p.credits), Some(1_000));
        });

        // Out-of-range selections are ignored rather than panicking.
        model.update(&mut app, |model, ctx| model.select_credit_pack(9, ctx));
        model.read(&app, |model, _| {
            assert_eq!(model.selected_credit_pack_index(), 1);
        });
    });
}

/// Regression test for REV-1886: browser checkout must not advance onboarding
/// on its own. The purchase stays in flight until the credits actually land,
/// so abandoning checkout leaves the user on the offer slide.
#[test]
fn abandoned_checkout_leaves_the_purchase_in_flight() {
    App::test((), |mut app| async move {
        let model = add_test_model(&mut app);
        model.update(&mut app, |model, ctx| {
            model.show_post_auth_offer(OfferVariant::ChooseHowToStart, ctx);
            model.set_credit_pack_options(credit_packs(), ctx);
            model.request_credit_purchase(ctx);
        });
        assert_eq!(
            purchase_state(&app, &model),
            CreditPurchaseState::Purchasing
        );

        model.update(&mut app, |model, ctx| model.on_credit_checkout_opened(ctx));
        assert_eq!(
            purchase_state(&app, &model),
            CreditPurchaseState::AwaitingCheckout
        );
        assert_eq!(step(&app, &model), OnboardingStep::PostAuthOffer);

        // Only the server reporting AI as available clears the in-flight
        // purchase.
        model.update(&mut app, |model, ctx| {
            model.on_credit_availability_observed(true, ctx)
        });
        assert_eq!(purchase_state(&app, &model), CreditPurchaseState::Idle);
    });
}

/// Regression test for REV-1886: cancelling browser checkout must leave the
/// user on the offer slide. The common case is a brand-new account that still
/// can't make an AI request, so every refresh while checkout is open reports
/// unavailable and the slide must hold.
#[test]
fn canceled_checkout_does_not_advance_a_user_without_ai_access() {
    App::test((), |mut app| async move {
        let model = add_test_model(&mut app);
        model.update(&mut app, |model, ctx| {
            model.show_post_auth_offer(OfferVariant::ChooseHowToStart, ctx);
            model.set_credit_pack_options(credit_packs(), ctx);
            model.request_credit_purchase(ctx);
            model.on_credit_checkout_opened(ctx);
        });

        // Every refresh while checkout is open still reports no AI access.
        for _ in 0..3 {
            model.update(&mut app, |model, ctx| {
                model.on_credit_availability_observed(false, ctx)
            });
            assert_eq!(
                purchase_state(&app, &model),
                CreditPurchaseState::AwaitingCheckout,
                "an unavailable answer must not complete the purchase"
            );
            assert_eq!(step(&app, &model), OnboardingStep::PostAuthOffer);
        }

        // Access arriving completes it.
        model.update(&mut app, |model, ctx| {
            model.on_credit_availability_observed(true, ctx)
        });
        assert_eq!(purchase_state(&app, &model), CreditPurchaseState::Idle);
    });
}

/// Onboarding doesn't care *how* the user ended up able to use AI — a team
/// plan landing mid-checkout counts just as much as the add-on credits they
/// were buying. The bar is "can make an AI request", not "this purchase
/// settled".
#[test]
fn access_arriving_from_any_source_completes_the_purchase() {
    App::test((), |mut app| async move {
        let model = add_test_model(&mut app);
        model.update(&mut app, |model, ctx| {
            model.show_post_auth_offer(OfferVariant::ChooseHowToStart, ctx);
            model.set_credit_pack_options(credit_packs(), ctx);
            model.request_credit_purchase(ctx);
            model.on_credit_checkout_opened(ctx);
            // Not the add-on credits: some other grant made AI usable.
            model.on_credit_availability_observed(true, ctx);
        });
        assert_eq!(purchase_state(&app, &model), CreditPurchaseState::Idle);
    });
}

/// The availability report rides along on a generic usage refresh, so it must
/// be inert while no AI-sell offer is on screen.
#[test]
fn observing_availability_outside_the_offer_does_nothing() {
    App::test((), |mut app| async move {
        let model = add_test_model(&mut app);
        let observer = observe_completions(&mut app, &model);
        model.update(&mut app, |model, ctx| {
            model.set_credit_pack_options(credit_packs(), ctx);
            model.on_credit_availability_observed(true, ctx);
        });
        assert_eq!(purchase_state(&app, &model), CreditPurchaseState::Idle);

        model.update(&mut app, |model, ctx| {
            model.request_credit_purchase(ctx);
            model.on_credit_availability_observed(true, ctx);
        });
        assert_eq!(
            purchase_state(&app, &model),
            CreditPurchaseState::Purchasing
        );
        assert_eq!(completions(&app, &observer), 0);
    });
}

/// Regression test for REV-1952: the user leaves the offer through the plan
/// call to action and buys a one-time pack on the web instead, so no
/// client-side checkout was ever recorded. Completion has to come from the
/// account having AI, not from a purchase the client started.
#[test]
fn credit_availability_advances_the_offer_without_a_pending_checkout() {
    App::test((), |mut app| async move {
        let model = add_test_model(&mut app);
        let observer = observe_completions(&mut app, &model);
        model.update(&mut app, |model, ctx| {
            model.show_post_auth_offer(OfferVariant::ChooseHowToStart, ctx);
            model.set_credit_pack_options(credit_packs(), ctx);
        });
        assert_eq!(purchase_state(&app, &model), CreditPurchaseState::Idle);

        model.update(&mut app, |model, ctx| {
            model.on_credit_availability_observed(false, ctx)
        });
        assert_eq!(
            completions(&app, &observer),
            0,
            "a user who still can't use AI must stay on the offer"
        );

        model.update(&mut app, |model, ctx| {
            model.on_credit_availability_observed(true, ctx)
        });
        assert_eq!(completions(&app, &observer), 1);
    });
}

/// Regression test for REV-1952: following the confirmation page's link back
/// into the app advances onboarding, so the flow no longer stalls on the offer
/// while the credit grant catches up.
#[test]
fn the_checkout_success_handoff_advances_the_ai_sell_offer() {
    App::test((), |mut app| async move {
        let model = add_test_model(&mut app);
        let observer = observe_completions(&mut app, &model);
        model.update(&mut app, |model, ctx| {
            model.show_post_auth_offer(OfferVariant::ChooseHowToStart, ctx);
        });

        let advanced = model.update(&mut app, |model, ctx| model.on_checkout_succeeded(ctx));
        assert!(advanced);
        assert_eq!(completions(&app, &observer), 1);
    });
}

/// The hand-off arrives on a generic deeplink, so it must be inert anywhere the
/// user isn't being sold AI: before the offer is shown, and on the head-start
/// offer, whose account already includes AI usage.
#[test]
fn the_checkout_success_handoff_is_inert_outside_an_ai_sell_offer() {
    App::test((), |mut app| async move {
        let model = add_test_model(&mut app);
        let observer = observe_completions(&mut app, &model);

        let advanced = model.update(&mut app, |model, ctx| model.on_checkout_succeeded(ctx));
        assert!(!advanced, "no offer is showing yet");

        model.update(&mut app, |model, ctx| {
            model.show_post_auth_offer(OfferVariant::HeadStart, ctx);
        });
        let advanced = model.update(&mut app, |model, ctx| model.on_checkout_succeeded(ctx));
        assert!(!advanced, "the head-start offer is not selling AI usage");

        model.update(&mut app, |model, ctx| {
            model.on_credit_availability_observed(true, ctx)
        });
        assert_eq!(completions(&app, &observer), 0);
    });
}

#[test]
fn a_synchronous_purchase_completes_without_checkout() {
    App::test((), |mut app| async move {
        let model = add_test_model(&mut app);
        model.update(&mut app, |model, ctx| {
            model.show_post_auth_offer(OfferVariant::ChooseHowToStart, ctx);
            model.set_credit_pack_options(credit_packs(), ctx);
            model.request_credit_purchase(ctx);
            model.on_credit_purchase_completed(ctx);
        });
        assert_eq!(purchase_state(&app, &model), CreditPurchaseState::Idle);
    });
}

#[test]
fn a_rejected_purchase_is_retryable() {
    App::test((), |mut app| async move {
        let model = add_test_model(&mut app);
        model.update(&mut app, |model, ctx| {
            model.set_credit_pack_options(credit_packs(), ctx);
            model.request_credit_purchase(ctx);
            model.on_credit_purchase_failed(ctx);
        });
        assert_eq!(purchase_state(&app, &model), CreditPurchaseState::Failed);

        model.update(&mut app, |model, ctx| model.request_credit_purchase(ctx));
        assert_eq!(
            purchase_state(&app, &model),
            CreditPurchaseState::Purchasing
        );
    });
}

#[test]
fn a_purchase_cannot_start_without_packs_or_while_one_is_in_flight() {
    App::test((), |mut app| async move {
        let model = add_test_model(&mut app);

        // No packs offered yet: nothing to buy.
        model.update(&mut app, |model, ctx| model.request_credit_purchase(ctx));
        assert_eq!(purchase_state(&app, &model), CreditPurchaseState::Idle);

        model.update(&mut app, |model, ctx| {
            model.set_credit_pack_options(credit_packs(), ctx);
            model.request_credit_purchase(ctx);
            model.on_credit_checkout_opened(ctx);
            // A second request must not restart checkout...
            model.request_credit_purchase(ctx);
            // ...and the pack being paid for must not change underneath it.
            model.select_credit_pack(1, ctx);
        });
        assert_eq!(
            purchase_state(&app, &model),
            CreditPurchaseState::AwaitingCheckout
        );
        model.read(&app, |model, _| {
            assert_eq!(model.selected_credit_pack_index(), 0);
        });
    });
}

/// Completion callbacks are safe to fire speculatively (they are driven by a
/// generic usage refresh), so they must be inert when nothing was purchased.
#[test]
fn purchase_callbacks_are_inert_when_no_purchase_is_in_flight() {
    App::test((), |mut app| async move {
        let model = add_test_model(&mut app);
        model.update(&mut app, |model, ctx| {
            model.set_credit_pack_options(credit_packs(), ctx);
            model.on_credit_purchase_completed(ctx);
            model.on_credit_checkout_opened(ctx);
            model.on_credit_purchase_failed(ctx);
        });
        assert_eq!(purchase_state(&app, &model), CreditPurchaseState::Idle);
    });
}

#[test]
fn credit_pack_labels_are_formatted_for_display() {
    let pack = CreditPackOption {
        credits: 6_500,
        price_usd_cents: 12_000,
        savings_percent: 38,
    };
    assert_eq!(pack.credits_label(), "6,500");
    assert_eq!(pack.price_label(), "$120");

    let fractional = CreditPackOption {
        credits: 400,
        price_usd_cents: 1_250,
        savings_percent: 0,
    };
    assert_eq!(fractional.credits_label(), "400");
    assert_eq!(fractional.price_label(), "$12.50");
}

#[test]
fn agent_intent_keeps_ai_enabled_for_any_setup_choice() {
    App::test((), |mut app| async move {
        let model = add_test_model(&mut app);

        // Default agent intention + "Use Warp Agent" enables AI.
        model.read(&app, |model, _| assert!(model.settings().is_ai_enabled()));

        // "Use third party agents" still keeps AI enabled: agent intent always
        // means the user wants AI, even when bringing their own agents.
        model.update(&mut app, |model, ctx| {
            model.set_ai_setup_choice(AiSetupChoice::ThirdParty, ctx)
        });
        model.read(&app, |model, _| assert!(model.settings().is_ai_enabled()));

        // Switching back to Warp Agent also keeps AI enabled.
        model.update(&mut app, |model, ctx| {
            model.set_ai_setup_choice(AiSetupChoice::WarpAgent, ctx)
        });
        model.read(&app, |model, _| assert!(model.settings().is_ai_enabled()));
    });
}

