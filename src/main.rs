use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use openxr::{self as xr};
use openxr_mndx_xdev_space::SessionXDevExtensionMNDX;
use openxr_mndx_xdev_space::XR_MNDX_XDEV_SPACE_EXTENSION_NAME;

use std::net::{UdpSocket};

use rosc::encoder;
use rosc::{OscMessage, OscPacket, OscType};

use std::time::{SystemTime};

use std::env;

struct TrackedThing{
    name: String,
    space: xr::Space,
}


pub fn main() {
    // Handle interrupts gracefully
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        r.store(false, Ordering::Relaxed);
    })
    .expect("setting Ctrl-C handler");

    #[cfg(feature = "static")]
    let entry = xr::Entry::linked();
    #[cfg(not(feature = "static"))]
    let entry = unsafe {
        xr::Entry::load()
            .expect("couldn't find the OpenXR loader; try enabling the \"static\" feature")
    };

    let mut port = "27769".to_owned();
    // Handle command-line args
    let args: Vec<String> = env::args().collect();
    if args.len() > 1 {
        for i in 1..args.len() {
            if args[i] == "-port" && i < args.len()-1{
                println!("PORT: {}", args[i+1]);
                port = args[i+1].clone();
            }
        }
    }

    let osc_dest = "127.0.0.1:".to_owned() + &port;
    println!("DEST {}", osc_dest);


    let mut xdev_ext = XR_MNDX_XDEV_SPACE_EXTENSION_NAME.as_bytes().to_vec();
    xdev_ext.push(0);
    // OpenXR will fail to initialize if we ask for an extension that OpenXR can't provide! So we
    // need to check all our extensions before initializing OpenXR with them. Note that even if the
    // extension is present, it's still possible you may not be able to use it. For example: the
    // hand tracking extension may be present, but the hand sensor might not be plugged in or turned
    // on. There are often additional checks that should be made before using certain features!
    let available_extensions = entry.enumerate_extensions().unwrap();
    

    // If a required extension isn't present, you want to ditch out here! It's possible something
    // like your rendering API might not be provided by the active runtime. APIs like OpenGL don't
    // have universal support.
    assert!(available_extensions.mnd_headless);
    //assert!(has_xdev_extension);

    // Initialize OpenXR with the extensions we've found!
    let mut enabled_extensions = xr::ExtensionSet::default();
    enabled_extensions.mnd_headless = true;
    enabled_extensions.other
                .push(xdev_ext);

    
    let xr_instance = entry
        .create_instance(
            &xr::ApplicationInfo {
                application_name: "XDev-to-Nyan",
                application_version: 0,
                engine_name: "openxrs example",
                engine_version: 0,
                api_version: xr::Version::new(1, 0, 0),
            },
            &enabled_extensions,
            &[],
        )
        .unwrap();
    let instance_props = xr_instance.properties().unwrap();
    println!(
        "loaded OpenXR runtime: {} {}",
        instance_props.runtime_name, instance_props.runtime_version
    );

    // Request a form factor from the device (HMD, Handheld, etc.)
    let system = xr_instance
        .system(xr::FormFactor::HEAD_MOUNTED_DISPLAY)
        .unwrap();


    let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
    let starting_time = SystemTime::now();


    #[allow(clippy::missing_transmute_annotations)]
    unsafe {
        // A session represents this application's desire to display things! This is where we hook
        // up our graphics API. This does not start the session; for that, you'll need a call to
        // Session::begin, which we do in 'main_loop below.
        let (session, mut frame_wait, frame_stream) = xr_instance
            .create_session::<xr::Headless>(
                system,
                &xr::headless::SessionCreateInfo {},
            )
            .unwrap();

        // // Create an action set to encapsulate our actions
        // let action_set = xr_instance
        //     .create_action_set("input", "input pose information", 0)
        //     .unwrap();

        // let right_action = action_set
        //     .create_action::<xr::Posef>("right_hand", "Right Hand Controller", &[])
        //     .unwrap();
        // let left_action = action_set
        //     .create_action::<xr::Posef>("left_hand", "Left Hand Controller", &[])
        //     .unwrap();

        // // Bind our actions to input devices using the given profile
        // // If you want to access inputs specific to a particular device you may specify a different
        // // interaction profile
        // xr_instance
        //     .suggest_interaction_profile_bindings(
        //         xr_instance
        //             .string_to_path("/interaction_profiles/khr/simple_controller")
        //             .unwrap(),
        //         &[
        //             xr::Binding::new(
        //                 &right_action,
        //                 xr_instance
        //                     .string_to_path("/user/hand/right/input/grip/pose")
        //                     .unwrap(),
        //             ),
        //             xr::Binding::new(
        //                 &left_action,
        //                 xr_instance
        //                     .string_to_path("/user/hand/left/input/grip/pose")
        //                     .unwrap(),
        //             ),
        //         ],
        //     )
        //     .unwrap();

        // // Attach the action set to the session
        // session.attach_action_sets(&[&action_set]).unwrap();


        let mut things:Vec<TrackedThing> = Vec::new();
        // // Create an action space for each device we want to locate
        // let right_space = right_action
        //     .create_space(&session, xr::Path::NULL, xr::Posef::IDENTITY)
        //     .unwrap();
        // let left_space = left_action
        //     .create_space(&session, xr::Path::NULL, xr::Posef::IDENTITY)
        //     .unwrap();

        session.get_xdev_list().unwrap().enumerate_xdevs().unwrap().into_iter().for_each(|xd| {
                if xd.can_create_space() {
                    let spac = xd.create_space(xr::Posef::IDENTITY).unwrap();

                    things.push(TrackedThing{ name: xd.serial().to_string(), space: spac});
                    println!("XDev - Name: {}, Serial: {}", xd.name(), xd.serial())
                }
            });



        // OpenXR uses a couple different types of reference frames for positioning content; we need
        // to choose one for displaying our content! STAGE would be relative to the center of your
        // guardian system's bounds, and LOCAL would be relative to your device's starting location.
        let stage = session
            .create_reference_space(xr::ReferenceSpaceType::STAGE, xr::Posef::IDENTITY)
            .unwrap();

        // Main loop
        
        let mut event_storage = xr::EventDataBuffer::new();
        let mut session_running = false;
        // Index of the current frame, wrapped by PIPELINE_DEPTH. Not to be confused with the
        // swapchain image index.
        let mut frame = 0;
        'main_loop: loop {
            if !running.load(Ordering::Relaxed) {
                println!("requesting exit");
                // The OpenXR runtime may want to perform a smooth transition between scenes, so we
                // can't necessarily exit instantly. Instead, we must notify the runtime of our
                // intent and wait for it to tell us when we're actually done.
                match session.request_exit() {
                    Ok(()) => {}
                    Err(xr::sys::Result::ERROR_SESSION_NOT_RUNNING) => break,
                    Err(e) => panic!("{}", e),
                }
            }

            while let Some(event) = xr_instance.poll_event(&mut event_storage).unwrap() {
                use xr::Event::*;
                match event {
                    SessionStateChanged(e) => {
                        // Session state change is where we can begin and end sessions, as well as
                        // find quit messages!
                        println!("entered state {:?}", e.state());
                        match e.state() {
                            xr::SessionState::READY => {
                                session.begin(VIEW_TYPE).unwrap();
                                session_running = true;
                            }
                            xr::SessionState::STOPPING => {
                                session.end().unwrap();
                                session_running = false;
                            }
                            xr::SessionState::EXITING | xr::SessionState::LOSS_PENDING => {
                                break 'main_loop;
                            }
                            _ => {}
                        }
                    }
                    InstanceLossPending(_) => {
                        break 'main_loop;
                    }
                    EventsLost(e) => {
                        println!("lost {} events", e.lost_event_count());
                    }
                    _ => {}
                }
            }

            if !session_running {
                // Don't grind up the CPU
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }


            // // Block until the previous frame is finished displaying, and is ready for another one.
            // // Also returns a prediction of when the next frame will be displayed, for use with
            // // predicting locations of controllers, viewpoints, etc.
            let xr_frame_state = frame_wait.wait().unwrap();            

            //session.sync_actions(&[(&action_set).into()]).unwrap();

            // // Find where our controllers are located in the Stage space
            // let right_location = right_space
            //     .locate(&stage, xr_frame_state.predicted_display_time)
            //     .unwrap();

            // let left_location = left_space
            //     .locate(&stage, xr_frame_state.predicted_display_time)
            //     .unwrap();

            // if left_action.is_active(&session, xr::Path::NULL).unwrap() {
            //     print!(
            //         "Left Hand: ({:0<12},{:0<12},{:0<12}), ",
            //         left_location.pose.position.x,
            //         left_location.pose.position.y,
            //         left_location.pose.position.z
            //     );
            // }

            // if right_action.is_active(&session, xr::Path::NULL).unwrap() {
            //     print!(
            //         "Right Hand: ({:0<12},{:0<12},{:0<12})",
            //         right_location.pose.position.x,
            //         right_location.pose.position.y,
            //         right_location.pose.position.z
            //     );
            // }

            things.iter().for_each(|thing| {
                let location = thing.space.locate(&stage, xr_frame_state.predicted_display_time).unwrap();

                let msg_buf = encoder::encode(&OscPacket::Message(OscMessage {
                    addr: "/vnyan/tracker/pos".to_string(),
                    args: vec![OscType::String(thing.name.clone()), 
                    OscType::Float(location.pose.position.x),
                    OscType::Float(location.pose.position.y),
                    OscType::Float(-location.pose.position.z),
                    OscType::Float(-location.pose.orientation.x),
                    OscType::Float(-location.pose.orientation.y),
                    OscType::Float(location.pose.orientation.z),
                    OscType::Float(location.pose.orientation.w)],
                    }))
                    .unwrap();

                    sock.send_to(&msg_buf, &osc_dest).unwrap();
            });

            let msg_buf = encoder::encode(&OscPacket::Message(OscMessage {
                    addr: "/vnyan/timestamp".to_string(),
                    args: vec![OscType::Float(SystemTime::now().duration_since(starting_time).expect("Time thing").as_secs_f32())],}))
                    .unwrap();

            sock.send_to(&msg_buf, &osc_dest).unwrap();


            std::thread::sleep(Duration::from_millis(1000/90));
            
            frame = (frame + 1) % PIPELINE_DEPTH as usize;
        }


        
        // OpenXR MUST be allowed to clean up before we destroy Vulkan resources it could touch, so
        // first we must drop all its handles.
        drop((
            session,
            frame_wait,
            frame_stream,
            stage,
            // action_set,
            // left_space,
            // right_space,
            // left_action,
            // right_action,
        ));

    }

    println!("exiting cleanly");
}


pub const VIEW_COUNT: u32 = 2;
const VIEW_TYPE: xr::ViewConfigurationType = xr::ViewConfigurationType::PRIMARY_STEREO;

/// Maximum number of frames in flight
const PIPELINE_DEPTH: u32 = 2;
