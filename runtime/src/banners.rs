use support::{decl_storage, decl_module, StorageValue, StorageMap,
    dispatch::Result, ensure, decl_event, traits::Currency};
use system::ensure_signed;
use runtime_primitives::traits::{As, Hash};
use parity_codec::{Encode, Decode};
use rstd::prelude::Vec;

const AUCTION_DURATION: u64 = 24*600;

#[derive(Encode, Decode, Default, Clone, PartialEq)]
#[cfg_attr(feature = "std", derive(Debug))]
pub struct Banner<Hash, Balance, AccountId, BlockNumber> {
    id: Hash,
    name: Vec<u8>,
    image_url: Vec<u8>,
    desc: Vec<u8>,
    current_price: Balance,
    current_bidder: AccountId,
    can_bid: bool,
    bid_end_height: BlockNumber,
}

pub trait Trait: balances::Trait {
    type Event: From<Event<Self>> + Into<<Self as system::Trait>::Event>;
}

decl_event!(
    pub enum Event<T>
    where
        <T as system::Trait>::AccountId,
        <T as system::Trait>::Hash,
        <T as balances::Trait>::Balance
    {
        CreateBanner(AccountId, Hash),
        StartAuction(AccountId, Hash, Balance),
        Bid(AccountId, Hash, Balance),
        Transferred(AccountId, AccountId, Hash),
        Deal(AccountId, Hash, Balance),
        Abort(AccountId, Hash),
    }
);

decl_storage! {
    trait Store for Module<T: Trait> as BannerStorage {
        Banners get(banner): map T::Hash => Banner<T::Hash, T::Balance, T::AccountId, T::BlockNumber>;
        BannerOwner get(owner_of): map T::Hash => Option<T::AccountId>;

        AllBannersArray get(banner_by_index): map u64 => T::Hash;
        AllBannersCount get(all_banners_count): u64;
        AllBannersIndex: map T::Hash => u64;

        OwnedBannersArray get(banner_of_owner_by_index): map (T::AccountId, u64) => T::Hash;
        OwnedBannersCount get(owned_banner_count): map T::AccountId => u64;
        OwnedBannersIndex: map T::Hash => u64;

        Nonce: u64;
    }
}

decl_module! {
    pub struct Module<T: Trait> for enum Call where origin: T::Origin {
        
        fn deposit_event<T>() = default;

        fn create_banner(origin, name: Vec<u8>, url: Vec<u8>, desc: Vec<u8>) -> Result {
            let sender = ensure_signed(origin)?;
            let nonce = <Nonce<T>>::get();
            let random_hash = (<system::Module<T>>::random_seed(), &sender, nonce)
                .using_encoded(<T as system::Trait>::Hashing::hash);

            let new_banner = Banner {
                id: random_hash,
                name: name,
                image_url: url,
                desc: desc,
                current_price: <T::Balance as As<u64>>::sa(0),
                current_bidder:  sender.clone(),
                bid_end_height: <T::BlockNumber as As<u64>>::sa(0),
                can_bid: false,
            };

            Self::mint(sender, random_hash, new_banner)?;

            <Nonce<T>>::mutate(|n| *n += 1);

            Ok(())
        }

        fn set_image_url(origin, banner_id: T::Hash, new_url: Vec<u8>) -> Result {
            let sender = ensure_signed(origin)?;

            ensure!(<Banners<T>>::exists(banner_id), "This banner does not exist");

            let owner = Self::owner_of(banner_id).ok_or("No owner for this banner")?;
            ensure!(owner == sender, "You do not own this banner");

            let mut banner = Self::banner(banner_id);
            banner.image_url = new_url;

            <Banners<T>>::insert(banner_id, banner);

            Ok(())
        }

        fn auction_banner(origin, banner_id: T::Hash, starting_price: T::Balance) -> Result {
            let sender = ensure_signed(origin)?;

            ensure!(<Banners<T>>::exists(banner_id), "This banner does not exist");

            let owner = Self::owner_of(banner_id).ok_or("No owner for this banner")?;
            ensure!(owner == sender, "You do not own this banner");

            let mut banner = Self::banner(banner_id);
            ensure!(banner.can_bid == false, "This banner has already been auctioned");

            banner.current_price = starting_price;
            banner.can_bid = true;
            banner.current_bidder = sender.clone();
            banner.bid_end_height = <system::Module<T>>::block_number() + <T::BlockNumber as As<u64>>::sa(AUCTION_DURATION);
            
            <Banners<T>>::insert(banner_id, banner);

            Self::deposit_event(RawEvent::StartAuction(sender, banner_id, starting_price));

            Ok(())
        }

        fn bid(origin, banner_id: T::Hash, bid_price: T::Balance) -> Result {
            let sender = ensure_signed(origin)?;

            ensure!(<Banners<T>>::exists(banner_id), "This banner does not exist");
            let owner = Self::owner_of(banner_id).ok_or("No owner for this banner")?;

            let mut banner = Self::banner(banner_id);
            ensure!(banner.can_bid, "This banner can't be bid");

            if banner.bid_end_height > <system::Module<T>>::block_number() {
                // still can bid this banner
                ensure!(owner != sender, "You can't bid your own banner");
                ensure!(bid_price > banner.current_price, "your bid price must be greater than current price");

                <balances::Module<T> as Currency<_>>::transfer(&sender, &banner.current_bidder, banner.current_price)?;
                <balances::Module<T> as Currency<_>>::transfer(&sender, &owner, bid_price - banner.current_price)?;

                banner.current_bidder = sender.clone();
                banner.current_price = bid_price;

                <Banners<T>>::insert(banner_id, banner);

                Self::deposit_event(RawEvent::Bid(sender, banner_id, bid_price));

            }else {
                let final_price = banner.current_price;
                let final_bidder = banner.current_bidder;

                banner.can_bid = false;
                banner.bid_end_height = <T::BlockNumber as As<u64>>::sa(0);
                banner.current_bidder = final_bidder.clone();
                banner.current_price = <T::Balance as As<u64>>::sa(0);
                <Banners<T>>::insert(banner_id, banner);

                if final_bidder.clone() == owner {
                    // 流拍
                    Self::deposit_event(RawEvent::Abort(owner.clone(), banner_id));
                } else {
                    // 有效成交
                    Self::transfer_from(owner.clone(), final_bidder.clone(), banner_id)?;
                    Self::deposit_event(RawEvent::Deal(final_bidder, banner_id, final_price));
                }
            }

            Ok(())
        }

    }
}

impl<T: Trait> Module<T> {
    fn mint(to: T::AccountId, banner_id: T::Hash, new_banner: Banner<T::Hash, T::Balance, T::AccountId, T::BlockNumber>) -> Result {
        ensure!(!<BannerOwner<T>>::exists(banner_id), "banner already exists");

        let owned_banner_count = Self::owned_banner_count(&to);

        let new_owned_banner_count = owned_banner_count.checked_add(1)
            .ok_or("Overflow adding a new banner to account balance")?;

        let all_banners_count = Self::all_banners_count();

        let new_all_banners_count = all_banners_count.checked_add(1)
            .ok_or("Overflow adding a new banner to total supply")?;

        <Banners<T>>::insert(banner_id, new_banner);
        <BannerOwner<T>>::insert(banner_id, &to);

        <AllBannersArray<T>>::insert(all_banners_count, banner_id);
        <AllBannersCount<T>>::put(new_all_banners_count);
        <AllBannersIndex<T>>::insert(banner_id, all_banners_count);

        <OwnedBannersArray<T>>::insert((to.clone(), owned_banner_count), banner_id);
        <OwnedBannersCount<T>>::insert(&to, new_owned_banner_count);
        <OwnedBannersIndex<T>>::insert(banner_id, owned_banner_count);

        Self::deposit_event(RawEvent::CreateBanner(to, banner_id));

        Ok(())
    }

    fn transfer_from(from: T::AccountId, to: T::AccountId, banner_id: T::Hash) -> Result {
        let owner = Self::owner_of(banner_id).ok_or("No owner for this banner")?;

        ensure!(owner == from, "'from' account does not own this banner");

        let owned_banner_count_from = Self::owned_banner_count(&from);
        let owned_banner_count_to = Self::owned_banner_count(&to);

        let new_owned_banner_count_to = owned_banner_count_to.checked_add(1)
            .ok_or("Transfer causes overflow of 'to' banner balance")?;
        let new_owned_banner_count_from = owned_banner_count_from.checked_sub(1)
            .ok_or("Transfer causes underflow of 'from' banner balance")?;

        let banner_index = <OwnedBannersIndex<T>>::get(banner_id);
        if banner_index != new_owned_banner_count_from {
            let last_banner_id = <OwnedBannersArray<T>>::get((from.clone(), new_owned_banner_count_from));
            <OwnedBannersArray<T>>::insert((from.clone(), banner_index), last_banner_id);
            <OwnedBannersIndex<T>>::insert(last_banner_id, banner_index);
        }

        <BannerOwner<T>>::insert(&banner_id, &to);
        <OwnedBannersIndex<T>>::insert(banner_id, owned_banner_count_to);

        <OwnedBannersArray<T>>::remove((from.clone(), new_owned_banner_count_from));
        <OwnedBannersArray<T>>::insert((to.clone(), owned_banner_count_to), banner_id);

        <OwnedBannersCount<T>>::insert(&from, new_owned_banner_count_from);
        <OwnedBannersCount<T>>::insert(&to, new_owned_banner_count_to);

        Self::deposit_event(RawEvent::Transferred(from, to, banner_id));

        Ok(())
    }
}