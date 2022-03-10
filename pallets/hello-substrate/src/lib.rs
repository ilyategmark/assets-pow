#![cfg_attr(not(feature = "std"), no_std)]
#![allow(clippy::unused_unit)]
//! A pallet to demonstrate usage of a simple storage map
//!
//! Storage maps map a key type to a value type. The hasher used to hash the key can be customized.
//! This pallet uses the `blake2_128_concat` hasher. This is a good default hasher.

pub use pallet::*;

#[frame_support::pallet]
pub mod pallet {
	use frame_support::traits::{ExistenceRequirement, Currency};
	use frame_support::{dispatch::DispatchResult, pallet_prelude::*};
	use frame_system::ensure_signed;
	use frame_system::pallet_prelude::*;
	use sp_runtime::print;
	use sp_std::prelude::*;

	type BalanceOf<T> =
		<<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

	// Struct for holding asset information
	#[derive(Clone, Encode, Decode, PartialEq, RuntimeDebug)]
	pub struct Asset<T: Config> {
		pub asset_id: u32,
		// `None` assumes not for sale
		pub price: Option<BalanceOf<T>>,
		pub owner: T::AccountId,
	}

	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// Because this pallet emits events, it depends on the runtime's definition of an event.
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;

		/// The Currency handler for the kitties pallet.
		type Currency: Currency<Self::AccountId>;

		/// The maximum amount of assets a single account can own.
		#[pallet::constant]
		type MaxAssetsOwned: Get<u32>;

		/// The maximum number of assets in a blockchain
		#[pallet::constant]
		type MaxNumberOfAssets: Get<u32>;
	}

	// Errors inform users that something went wrong.
	#[pallet::error]
	pub enum Error<T> {
		/// An account reached maximum of assets owned
		TooManyOwned,
		/// Trying to transfer or buy an asset from oneself.
		TransferToSelf,
		/// This kitty already exists!
		DuplicateKitty,
		/// This asset already exists!
		DuplicateAsset,
		/// This kitty does not exist!
		NoKitty,
		/// This asset does not exist!
		NoAsset,
		/// You are not the owner of this asset.
		NotOwner,
		/// This asset is not for sale.
		NotForSale,
		/// Ensures that the buying price is greater than the asking price.
		BidPriceTooLow,
		/// You need to have two cats with different gender to breed.
		CantBreed,
		/// This person doesn't have enough resources to buy this land asset
		NotEnoughMoney,
		/// The owner of this asset doesn't intend to sell it
		AssetIsntForSale,
		/// While you waited, the asset was gone
		AssetAlreadySold,
		/// While you waited, the price has changed
		PriceChanged,
		/// Addition Overflow
		AdditionOverflow,
	}

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// A new asset was successfully created.
		AssetCreated { asset_id: u32, owner: T::AccountId },
		/// The price of an asset was successfully set.
		AssetPriceSet {
			asset_id: u32,
			price: Option<BalanceOf<T>>,
		},
		/// An asset was successfully transferred.
		AssetTransferred {
			from: T::AccountId,
			to: T::AccountId,
			asset_id: u32,
		},
		/// An asset was successfully sold.
		AssetSold {
			seller: T::AccountId,
			buyer: T::AccountId,
			asset_id: u32,
			price: BalanceOf<T>,
		},
	}

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	pub struct Pallet<T>(PhantomData<T>);

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {}

	/// Keeps track of the number of assets in existence.
	#[pallet::storage]
	#[pallet::getter(fn get_count_of_assets)]
	pub(super) type CountForAssets<T: Config> = StorageValue<_, u64, ValueQuery>;

	/// Maps the asset struct to the asset id.
	#[pallet::storage]
	#[pallet::getter(fn get_assets)]
	pub(super) type Assets<T: Config> = StorageMap<_, Twox64Concat, u32, Asset<T>>;

	/// Track the assets owned by each account.
	#[pallet::storage]
	#[pallet::getter(fn get_count_of_assets_owned)]
	pub(super) type AssetsOwned<T: Config> =
		StorageMap<_, Twox64Concat, T::AccountId, Vec<u32>, ValueQuery>;

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// Create a new asset.
		/// The actual asset creation is done in the `mintasset()` function.
		#[pallet::weight(0)]
		pub fn create_asset(origin: OriginFor<T>, asset_id: u32) -> DispatchResultWithPostInfo {
			// Make sure the caller is from a signed origin
			let sender = ensure_signed(origin)?;

			// Write new asset to storage by calling helper function
			Self::mintasset(&sender, asset_id)?;

			Ok(().into())
		}

		/// Directly transfer an asset to another recipient.
		/// Any account that holds an asset can send it to another account. This will reset the
		/// asking price of the asset, marking it not for sale.
		#[pallet::weight(0)]
		pub fn asset_transfer(
			origin: OriginFor<T>,
			to: T::AccountId,
			asset_id: u32,
		) -> DispatchResultWithPostInfo {
			// Make sure the caller is from a signed origin
			let from = ensure_signed(origin)?;
			let asset = Assets::<T>::get(&asset_id).ok_or(Error::<T>::NoAsset)?;
			ensure!(asset.owner == from, Error::<T>::NotOwner);
			Self::do_transfer_asset(asset_id, to, None)?;

			Ok(().into())
		}

		/// Buy a saleable asset. The bid price provided from the buyer has to be equal or higher
		/// than the ask price from the seller.
		/// This will reset the asking price of the asset, marking it not for sale.
		/// Marking this method `transactional` so when an error is returned, we ensure no storage
		/// is changed.
		#[pallet::weight(0)]
		pub fn buy_asset(
			origin: OriginFor<T>,
			asset_id: u32,
			bid_price: BalanceOf<T>,
		) -> DispatchResultWithPostInfo {
			// Make sure the caller is from a signed origin
			let buyer = ensure_signed(origin)?;
			// Transfer the asset from seller to buyer as a sale.
			Self::do_transfer_asset(asset_id, buyer, Some(bid_price))?;

			Ok(().into())
		}

		/// Set the price for an asset.
		/// Updates asset price and updates storage.
		#[pallet::weight(0)]
		pub fn set_asset_price(
			origin: OriginFor<T>,
			asset_id: u32,
			new_price: Option<BalanceOf<T>>,
		) -> DispatchResultWithPostInfo {
			// Make sure the caller is from a signed origin
			let sender = ensure_signed(origin)?;

			// Ensure the asset exists and is called by the asset owner
			let mut asset = Assets::<T>::get(&asset_id).ok_or(Error::<T>::NoAsset)?;
			ensure!(asset.owner == sender, Error::<T>::NotOwner);

			// Set the price in storage
			asset.price = new_price;
			Assets::<T>::insert(&asset_id, asset);

			// Deposit a "PriceSet" event.
			Self::deposit_event(Event::AssetPriceSet {
				asset_id,
				price: new_price,
			});

			Ok(().into())
		}
	}

	//** Our helper functions.**//

	impl<T: Config> Pallet<T> {
		// Helper to mint an asset
		pub fn mintasset(owner: &T::AccountId, asset_id: u32) -> Result<u32, DispatchError> {
			// Create a new asset

			let asset = Asset::<T> {
				asset_id,
				price: None,
				owner: owner.clone(),
			};

			// Check if the asset does not already exist in our storage map
			ensure!(
				!Assets::<T>::contains_key(&asset.asset_id),
				Error::<T>::DuplicateAsset
			);

			// Performs this operation first as it may fail
			let count = CountForAssets::<T>::get();
			let new_count = count.checked_add(1).ok_or(Error::<T>::AdditionOverflow)?;

			// Append asset to AssetsOwned

			AssetsOwned::<T>::append(&owner, asset.asset_id); // try_append was here when it was BoundedVec
			//	.map_err(|_| Error::<T>::TooManyOwned)?; // this check is probably no longer needed

			// Write new asset to storage
			Assets::<T>::insert(asset.asset_id, asset);
			CountForAssets::<T>::put(new_count);

			// Deposit our "Created" event.
			Self::deposit_event(Event::AssetCreated {
				asset_id,
				owner: owner.clone(),
			});

			// Returns the id of the new asset if this succeeds
			Ok(asset_id)
		}

		// Update storage to transfer asset
		pub fn do_transfer_asset(
			asset_id: u32,
			to: T::AccountId,
			maybe_bid_price: Option<BalanceOf<T>>,
		) -> DispatchResult {
			// Get the asset
			let mut asset = Assets::<T>::get(&asset_id).ok_or(Error::<T>::NoAsset)?;
			let from = asset.owner;

			ensure!(from != to, Error::<T>::TransferToSelf);
			let mut from_owned = AssetsOwned::<T>::get(&from);

			// Remove asset from the list of owned assets.
			if let Some(ind) = from_owned.iter().position(|&id| id == asset_id) {
				from_owned.swap_remove(ind);
			} else {
				return Err(Error::<T>::NoAsset.into());
			}

			// Add asset to the list of owned assets.
			let mut to_owned = AssetsOwned::<T>::get(&to);
			to_owned
				.push(asset_id); // try_push was here when it was BoundedVec
				//.map_err(|()| Error::<T>::TooManyOwned)?; it's no longer bounded vec, so this check is probably unnecessary

			// Mutating state here via a balance transfer, so nothing is allowed to fail after this.
			if let Some(bid_price) = maybe_bid_price {
				if let Some(price) = asset.price {
					ensure!(bid_price >= price, Error::<T>::BidPriceTooLow);
					// Transfer the amount from buyer to seller
					T::Currency::transfer(&to, &from, price, ExistenceRequirement::KeepAlive)?;
					// Deposit sold event
					Self::deposit_event(Event::AssetSold {
						seller: from.clone(),
						buyer: to.clone(),
						asset_id,
						price,
					});
				} else {
					return Err(Error::<T>::NotForSale.into());
				}
			}

			// Transfer succeeded, update the asset owner and reset the price to `None`.
			asset.owner = to.clone();
			asset.price = None;

			// Write updates to storage
			Assets::<T>::insert(&asset_id, asset);
			AssetsOwned::<T>::insert(&to, to_owned);
			AssetsOwned::<T>::insert(&from, from_owned);

			Self::deposit_event(Event::AssetTransferred { from, to, asset_id });

			Ok(())
		}
	}
}
