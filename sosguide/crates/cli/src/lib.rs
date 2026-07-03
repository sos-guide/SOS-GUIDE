//! Outils d'administration en ligne de commande : sonde de santé et chien de
//! garde matériel.
//!
//! La logique vit en bibliothèque (testable) ; le binaire `sos-cli` n'est qu'un
//! point d'entrée mince au-dessus de [`health`] et [`watchdog`].

pub mod health;
pub mod update;
pub mod watchdog;
